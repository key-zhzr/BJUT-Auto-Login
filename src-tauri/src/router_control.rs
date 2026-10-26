//! The App talks only to the named, authenticated rpcd methods shipped by the
//! router package. Remembered router passwords use the encrypted configuration backend; tokens stay in memory.
use crate::config_model::{RouterBinding, SavedRouter};
use base64::Engine;
use reqwest::{Client, Url};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const MAX_RESPONSE: usize = 2 * 1024 * 1024;
static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
struct PinnedCertificate {
    der: Vec<u8>,
    algorithms: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl rustls::client::danger::ServerCertVerifier for PinnedCertificate {
    fn verify_server_cert(
        &self,
        cert: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        // The user supplies the router's exact leaf certificate. Do not accept
        // other certificates from its CA or from the system roots in this mode.
        if cert.as_ref() != self.der {
            return Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::UnknownIssuer,
            ));
        }
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        signature: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, signature, &self.algorithms)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        signature: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, signature, &self.algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

fn router_client(certificate: Option<&str>) -> Result<Client, String> {
    let mut builder = Client::builder()
        .no_proxy()
        .use_rustls_tls()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10));
    if let Some(pem) = certificate.filter(|value| !value.trim().is_empty()) {
        use base64::Engine;
        if pem.len() > 32768 {
            return Err("路由器证书过大".into());
        }
        let encoded = pem
            .trim()
            .strip_prefix("-----BEGIN CERTIFICATE-----")
            .and_then(|text| text.strip_suffix("-----END CERTIFICATE-----"))
            .ok_or("请粘贴路由器的单个 PEM 证书")?;
        let encoded: String = encoded.chars().filter(|ch| !ch.is_whitespace()).collect();
        let der = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| "路由器证书格式无效")?;
        // Check the encoding even though trust is established by exact pinning.
        reqwest::Certificate::from_der(&der).map_err(|_| "路由器证书格式无效")?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = Arc::new(PinnedCertificate {
            der,
            algorithms: provider.signature_verification_algorithms,
        });
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|_| "无法准备安全连接")?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        builder = builder.use_preconfigured_tls(config);
    }
    builder.build().map_err(|_| "无法准备路由器连接".into())
}

fn connection_error(error: &reqwest::Error) -> String {
    let protocol = error
        .url()
        .map(|url| url.scheme())
        .unwrap_or("https")
        .to_ascii_uppercase();
    if error.is_timeout() {
        return "路由器连接超时，请检查地址与网络".into();
    }
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = cause {
        if let Some(io) = current.downcast_ref::<std::io::Error>() {
            match io.kind() {
                std::io::ErrorKind::ConnectionRefused => {
                    return format!("路由器未接受 {protocol} 连接，请确认服务已开启及端口正确")
                }
                std::io::ErrorKind::PermissionDenied => {
                    return "无法访问本地网络，请检查应用权限与路由器地址".into()
                }
                _ => {}
            }
        }
        if matches!(
            current.downcast_ref::<rustls::Error>(),
            Some(rustls::Error::InvalidCertificate(_))
        ) {
            return "ROUTER_CERT_UNTRUSTED:路由器证书尚未受信任，或已保存的证书发生变化".into();
        }
        cause = current
            .downcast_ref::<std::io::Error>()
            .and_then(std::io::Error::get_ref)
            .map(|inner| inner as &(dyn std::error::Error + 'static))
            .or_else(|| current.source());
    }
    "无法连接路由器，请检查网络、地址与端口".into()
}

// This verifier is used only for a certificate-only TLS handshake. There is
// no HTTP client, password, session token or application payload in this path.
#[derive(Debug)]
struct ReadOnlyCertificateProbe(PinnedCertificate);
impl rustls::client::danger::ServerCertVerifier for ReadOnlyCertificateProbe {
    fn verify_server_cert(
        &self,
        cert: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if cert.len() > 16384 {
            return Err(rustls::Error::General("certificate too large".into()));
        }
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        signature: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls12_signature(message, cert, signature)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        signature: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls13_signature(message, cert, signature)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct RouterCertificate {
    pem: String,
    fingerprint: String,
    address: String,
}

#[tauri::command]
pub(crate) async fn probe_router_certificate(address: String) -> Result<RouterCertificate, String> {
    use sha2::{Digest, Sha256};
    let url = endpoint(&address)?;
    if url.scheme() != "https" {
        return Err("请使用路由器 HTTPS 地址".into());
    }
    let host = url
        .host_str()
        .ok_or("路由器地址无效")?
        .trim_matches(['[', ']'])
        .to_owned();
    let port = url.port_or_known_default().ok_or("路由器端口无效")?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(ReadOnlyCertificateProbe(PinnedCertificate {
        der: Vec::new(),
        algorithms: provider.signature_verification_algorithms,
    }));
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| "无法准备证书读取")?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    let name =
        rustls::pki_types::ServerName::try_from(host.clone()).map_err(|_| "路由器地址无效")?;
    let der = tokio::time::timeout(Duration::from_secs(5), async {
        let socket = tokio::net::TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::ConnectionRefused {
                    "路由器未接受 HTTPS 连接，请确认 HTTPS 已开启及端口正确"
                } else {
                    "无法访问路由器，请检查本地网络权限与地址"
                }
            })?;
        let stream = tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(name, socket)
            .await
            .map_err(|_| "路由器 HTTPS 握手未完成，请检查端口与 TLS 设置")?;
        stream
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|certs| certs.first())
            .map(|cert| cert.as_ref().to_vec())
            .ok_or("路由器未提供证书")
    })
    .await
    .map_err(|_| "读取路由器证书超时，请检查网络")??;
    let fingerprint = Sha256::digest(&der)
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":");
    let encoded = base64::engine::general_purpose::STANDARD.encode(&der);
    let lines = encoded
        .as_bytes()
        .chunks(64)
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let mut origin = url;
    origin.set_path("/");
    Ok(RouterCertificate {
        pem: format!("-----BEGIN CERTIFICATE-----\n{lines}\n-----END CERTIFICATE-----\n"),
        fingerprint,
        address: origin.into(),
    })
}

#[derive(Default)]
pub(crate) struct Manager(Mutex<Option<Arc<Remote>>>, Mutex<Option<SavedRouter>>);

struct Remote {
    client: Client,
    endpoint: Url,
    token: String,
    router_id: Option<String>,
    expires: std::sync::Mutex<Instant>,
    lifetime: Duration,
    billing_tickets: std::sync::Mutex<std::collections::HashMap<String, Instant>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    version: String,
    uplinks: Vec<Value>,
    accounts: Vec<Value>,
    management_url: String,
    capabilities: Vec<String>,
    ports: Vec<Value>,
}

fn endpoint(value: &str) -> Result<Url, String> {
    endpoint_with_http(value, false)
}

fn private_http_host(host: &str) -> bool {
    host.trim_matches(['[', ']'])
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| match ip {
            std::net::IpAddr::V4(ip) => ip.is_private() || ip.is_loopback(),
            std::net::IpAddr::V6(ip) => ip.is_unique_local() || ip.is_loopback(),
        })
}

fn endpoint_with_http(value: &str, allow_http: bool) -> Result<Url, String> {
    let mut url = Url::parse(value.trim()).map_err(|_| "请填写完整的路由器地址")?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"));
    let lan_http = allow_http && url.host_str().is_some_and(private_http_host);
    if (url.scheme() != "https" && !(url.scheme() == "http" && (loopback || lan_http)))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/" | "/ubus" | "/ubus/")
    {
        return Err("请使用 HTTPS 地址；可信局域网的 HTTP 需先勾选允许选项".into());
    }
    url.set_path("/ubus");
    Ok(url)
}

fn valid_token(value: &str) -> bool {
    value.len() == 32
        && value.bytes().all(|b| b.is_ascii_hexdigit())
        && value.bytes().any(|b| b != b'0')
}

async fn rpc(
    client: &Client,
    url: &Url,
    token: &str,
    object: &str,
    method: &str,
    args: Value,
) -> Result<Value, String> {
    let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let request =
        json!({"jsonrpc":"2.0", "id":id, "method":"call", "params":[token, object, method, args]});
    let mut response = client
        .post(url.clone())
        .json(&request)
        .send()
        .await
        .map_err(|error| connection_error(&error))?;
    if !response.status().is_success() {
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err("路由管理接口未开启，请安装 uhttpd-mod-ubus 并启用 /ubus".into());
        }
        return Err(format!("路由器返回 HTTP {}", response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "路由器响应中断")? {
        if bytes.len() + chunk.len() > MAX_RESPONSE {
            return Err("路由器响应过大，请刷新后重试".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let body: Value =
        serde_json::from_slice(&bytes).map_err(|_| "路由器管理接口返回了无法识别的内容")?;
    if body.get("id").and_then(Value::as_u64) != Some(id) || body.get("error").is_some() {
        return Err("路由器拒绝了请求，请检查账号的管理权限".into());
    }
    let result = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or("路由器没有返回操作结果")?;
    match result.first().and_then(Value::as_u64) {
        // A successful rpcd session destroy has no response payload. uhttpd
        // supplies the current session itself and rejects it in caller args.
        Some(0) if object == "session" && method == "destroy" && result.len() == 1 => {
            Ok(Value::Null)
        }
        Some(0) => result
            .get(1)
            .cloned()
            .ok_or_else(|| "路由器没有返回操作数据".into()),
        Some(6) => Err("路由器登录已过期或权限不足，请重新连接".into()),
        Some(4) => Err("路由器未安装协作接口，请先更新路由端至 0.1.6".into()),
        _ => Err("路由器未完成请求，请稍后重试".into()),
    }
}

impl Remote {
    async fn call(&self, method: &str, args: Value) -> Result<Value, String> {
        if Instant::now() >= *self.expires.lock().unwrap() {
            return Err("路由器登录已过期，请重新连接".into());
        }
        // No token is sent until the public package identity has been checked.
        // Recheck on every operation: an identical LAN address on a new network
        // must not inherit the previous router's authenticated session.
        verify_router(&self.client, &self.endpoint, self.router_id.as_deref()).await?;
        let value = rpc(
            &self.client,
            &self.endpoint,
            &self.token,
            "bjut-autologin",
            method,
            args,
        )
        .await?;
        if value.get("state").and_then(Value::as_str) == Some("error") {
            return Err(value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("路由器操作未完成")
                .chars()
                .take(160)
                .collect());
        }
        *self.expires.lock().unwrap() = Instant::now() + self.lifetime;
        Ok(value)
    }

    async fn snapshot(&self) -> Result<Snapshot, String> {
        // Device IDs were introduced together with the combined dashboard API.
        // One RPC keeps the status, counters and account list in the same read.
        let combined = if self.router_id.is_some() {
            Some(self.call("dashboard", json!({})).await?)
        } else {
            None
        };
        let status = if let Some(data) = &combined {
            data["status"].clone()
        } else {
            self.call("status", json!({})).await?
        };
        if status.get("apiVersion").and_then(Value::as_u64) != Some(1) {
            return Err("路由端协作接口不兼容，请更新两端至 0.1.6".into());
        }
        let accounts = if let Some(data) = &combined {
            data["accounts"].clone()
        } else {
            self.call("accounts", json!({})).await?
        };
        let capabilities: Vec<String> = status
            .get("capabilities")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .take(32)
            .collect();
        let ports = if capabilities.iter().any(|item| item == "ports") {
            let data = if let Some(data) = &combined {
                data["ports"].clone()
            } else {
                self.call("ports", json!({})).await?
            };
            data.get("ports")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut management = self.endpoint.clone();
        management.set_path("/cgi-bin/luci/admin/network/bjut-autologin/billing");
        Ok(Snapshot {
            version: status
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("--")
                .to_owned(),
            uplinks: status
                .get("uplinks")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            accounts: accounts
                .get("accounts")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            management_url: management.into(),
            capabilities,
            ports,
        })
    }
}

async fn authenticate(profile: &SavedRouter) -> Result<(Arc<Remote>, Snapshot), String> {
    let endpoint = endpoint_with_http(&profile.address, profile.allow_http)?;
    if profile.username.trim().is_empty()
        || profile.username.len() > 128
        || profile.password.is_empty()
        || profile.password.len() > 1024
    {
        return Err("请填写路由器管理账号与密码".into());
    }
    let client = router_client(if endpoint.scheme() == "https" {
        profile.certificate.as_deref()
    } else {
        None
    })?;
    let router_id = verify_router(
        &client,
        &endpoint,
        profile.binding.as_ref().map(|b| b.router_id.as_str()),
    )
    .await?;
    let login = Box::pin(rpc(
        &client,
        &endpoint,
        "00000000000000000000000000000000",
        "session",
        "login",
        json!({"username":profile.username.trim(),"password":profile.password,"timeout":300}),
    ))
    .await?;
    let token = login
        .get("ubus_rpc_session")
        .and_then(Value::as_str)
        .filter(|token| valid_token(token))
        .ok_or("路由器账号或密码错误")?
        .to_owned();
    let timeout = login
        .get("timeout")
        .and_then(Value::as_u64)
        .unwrap_or(300)
        .clamp(10, 300);
    let remote = Remote {
        client,
        endpoint,
        token,
        router_id,
        expires: std::sync::Mutex::new(
            Instant::now() + Duration::from_secs(timeout.saturating_sub(5)),
        ),
        lifetime: Duration::from_secs(timeout.saturating_sub(5)),
        billing_tickets: std::sync::Mutex::new(std::collections::HashMap::new()),
    };
    let snapshot = Box::pin(remote.snapshot()).await?;
    Ok((Arc::new(remote), snapshot))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SavedConnection {
    address: String,
    username: String,
    certificate: Option<String>,
    allow_http: bool,
    auto_reconnect_ready: bool,
    network_name: String,
}

fn persist_router(
    app: &tauri::AppHandle,
    state: &crate::AppState,
    profile: Option<SavedRouter>,
) -> Result<(), String> {
    let mut config = state.config.write().unwrap();
    if config.saved_router == profile {
        return Ok(());
    }
    if state.credential_storage_status.lock().unwrap().as_str() != "available" {
        return Err("安全存储尚未就绪，暂时无法记住密码".into());
    }
    let mut updated = config.clone();
    updated.saved_router = profile;
    crate::save_secure_config_verified(app, &updated)?;
    *config = updated;
    Ok(())
}

#[tauri::command]
pub(crate) fn saved_router_connection(
    state: tauri::State<'_, Arc<crate::AppState>>,
) -> Option<SavedConnection> {
    state
        .config
        .read()
        .unwrap()
        .saved_router
        .as_ref()
        .map(|p| SavedConnection {
            address: p.address.clone(),
            username: p.username.clone(),
            certificate: p.certificate.clone(),
            allow_http: p.allow_http,
            auto_reconnect_ready: p.binding.is_some(),
            network_name: p
                .binding
                .as_ref()
                .map(|b| {
                    if b.ssid.is_empty() {
                        "有线网络".to_owned()
                    } else {
                        b.ssid.clone()
                    }
                })
                .unwrap_or_default(),
        })
}

#[tauri::command]
pub(crate) fn forget_router_password(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<crate::AppState>>,
) -> Result<(), String> {
    persist_router(&app, &state, None)
}

#[tauri::command]
// Keep the existing flat IPC fields; the first three arguments are injected by Tauri.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn connect_router(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<crate::AppState>>,
    manager: tauri::State<'_, Manager>,
    address: String,
    username: String,
    password: String,
    certificate: Option<String>,
    allow_http: Option<bool>,
    remember_password: Option<bool>,
) -> Result<Snapshot, String> {
    let mut profile = SavedRouter {
        address,
        username,
        password,
        certificate,
        allow_http: allow_http.unwrap_or(false),
        binding: None,
    };
    let mut binding = capture_network(&app).await?;
    let (remote, snapshot) = authenticate(&profile).await?;
    if let Some(id) = &remote.router_id {
        binding.router_id = id.clone();
        profile.binding = Some(binding);
    }
    if let Some(remember) = remember_password {
        persist_router(
            &app,
            &state,
            if remember {
                Some(profile.clone())
            } else {
                None
            },
        )?;
    }
    *manager.1.lock().await = Some(profile);
    *manager.0.lock().await = Some(remote);
    Ok(snapshot)
}

#[tauri::command]
pub(crate) async fn reconnect_router(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<crate::AppState>>,
    manager: tauri::State<'_, Manager>,
    remember_password: Option<bool>,
) -> Result<Snapshot, String> {
    let profile = manager
        .1
        .lock()
        .await
        .clone()
        .or_else(|| state.config.read().unwrap().saved_router.clone())
        .ok_or("请先连接路由器")?;
    let binding = profile
        .binding
        .as_ref()
        .ok_or("ROUTER_REPAIR_REQUIRED:请手动连接一次，以记住路由器和当前网络")?;
    let current = capture_network(&app).await?;
    if !same_network(binding, &current) {
        return Err("ROUTER_NETWORK_MISMATCH:当前网络与记住的路由器不匹配，已暂停自动连接".into());
    }
    let (remote, snapshot) = authenticate(&profile).await?;
    if let Some(remember) = remember_password {
        persist_router(
            &app,
            &state,
            if remember {
                Some(profile.clone())
            } else {
                None
            },
        )?;
    }
    *manager.1.lock().await = Some(profile);
    *manager.0.lock().await = Some(remote);
    Ok(snapshot)
}

#[tauri::command]
pub(crate) async fn get_router_status(
    manager: tauri::State<'_, Manager>,
) -> Result<Snapshot, String> {
    let remote = manager.0.lock().await.clone().ok_or("请先连接路由器")?;
    Box::pin(remote.snapshot()).await
}

#[tauri::command]
pub(crate) async fn router_login(
    manager: tauri::State<'_, Manager>,
    uplink: String,
    account: Option<String>,
) -> Result<String, String> {
    if !valid_section(&uplink)
        || account
            .as_ref()
            .is_some_and(|a| !a.is_empty() && !valid_section(a))
    {
        return Err("请重新选择上游与账号".into());
    }
    let remote = manager.0.lock().await.clone().ok_or("请先连接路由器")?;
    Box::pin(remote.call(
        "login",
        json!({"uplink":uplink,"account":account.unwrap_or_default()}),
    ))
    .await?;
    Ok("路由器已开始核对所选上游的登录状态".into())
}

fn valid_section(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

pub(crate) async fn sync(manager: &Manager, accounts: Vec<Value>) -> Result<String, String> {
    if accounts.is_empty() || accounts.len() > 31 {
        return Err("请选定 1 至 31 个已保存密码的账号".into());
    }
    let remote = manager.0.lock().await.clone().ok_or("请先连接路由器")?;
    Box::pin(remote.call("sync_accounts", json!({"payload":serde_json::to_string(&json!({"accounts":accounts})).map_err(|_| "账号内容无法读取")?}))).await?;
    Ok("选定账号已同步到路由器".into())
}

#[tauri::command]
pub(crate) async fn disconnect_router(manager: tauri::State<'_, Manager>) -> Result<(), String> {
    let remote = manager.0.lock().await.take();
    *manager.1.lock().await = None;
    if let Some(remote) = remote {
        if verify_router(
            &remote.client,
            &remote.endpoint,
            remote.router_id.as_deref(),
        )
        .await
        .is_err()
        {
            return Ok(());
        }
        let _ = Box::pin(rpc(
            &remote.client,
            &remote.endpoint,
            &remote.token,
            "session",
            "destroy",
            json!({}),
        ))
        .await;
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscoveredRouter {
    name: String,
    address: String,
    version: String,
    secure: bool,
}

fn discovery_origins(gateways: &[String], last_address: Option<&str>) -> Vec<Url> {
    let mut origins = Vec::new();
    if let Some(address) = last_address {
        if let Ok(mut url) = endpoint_with_http(address, true) {
            if url.host_str().is_some_and(|host| {
                private_http_host(host) && gateways.iter().any(|gateway| gateway == host)
            }) {
                url.set_path("/");
                origins.push(url);
            }
        }
    }
    for gateway in gateways.iter().take(16).filter(|ip| private_http_host(ip)) {
        let host = match gateway.parse::<std::net::IpAddr>() {
            Ok(std::net::IpAddr::V6(ip)) => format!("[{ip}]"),
            _ => gateway.to_owned(),
        };
        for scheme in ["https", "http"] {
            if let Ok(url) = Url::parse(&format!("{scheme}://{host}/")) {
                if !origins.contains(&url) {
                    origins.push(url);
                }
            }
        }
    }
    origins.truncate(32);
    origins
}

const NETWORK_MISMATCH: &str = "ROUTER_NETWORK_MISMATCH:当前路由器与记住的信息不符，已暂停自动连接";

async fn capture_network(app: &tauri::AppHandle) -> Result<RouterBinding, String> {
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let data = crate::get_network_info(app, Some(true));
        let text = |key| {
            data[key]
                .as_str()
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .chars()
                .take(128)
                .collect::<String>()
        };
        RouterBinding {
            router_id: String::new(),
            gateways: crate::network_inventory::router_gateways(),
            ssid: text("ssid"),
            bssid: text("bssid").to_ascii_lowercase().replace('-', ":"),
            transport: text("transport"),
        }
    })
    .await
    .map_err(|_| "无法核对当前网络".into())
}

fn same_network(saved: &RouterBinding, current: &RouterBinding) -> bool {
    // DHCP address changes are normal; do not bind to a client's exact IP.
    // An AP may roam within the same SSID. The router ID is checked separately
    // before any password or session token is sent.
    if !saved.ssid.is_empty() {
        return saved.ssid == current.ssid;
    }
    if !saved.bssid.is_empty() {
        return saved.bssid == current.bssid;
    }
    !saved.gateways.is_empty()
        && saved
            .gateways
            .iter()
            .any(|ip| current.gateways.contains(ip))
}

async fn router_metadata(client: &Client, origin: &Url) -> Result<Value, String> {
    let mut url = origin.clone();
    url.set_path("/.well-known/bjut-autologin.json");
    let mut response = client
        .get(url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map_err(|error| connection_error(&error))?;
    if response.status() != reqwest::StatusCode::OK {
        return Err("未发现路由端协作服务，请检查地址或更新路由端".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "路由标识读取中断")? {
        if bytes.len() + chunk.len() > 8192 {
            return Err("路由标识无效".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let data: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "未发现路由端协作服务，请检查地址或更新路由端")?;
    if data["product"] != "BJUT-Auto-Login-OpenWrt"
        || data["apiVersion"] != 1
        || data["apiPath"] != "/ubus"
    {
        return Err("未发现路由端协作服务，请检查地址或更新路由端".into());
    }
    Ok(data)
}

async fn verify_router(
    client: &Client,
    endpoint: &Url,
    expected: Option<&str>,
) -> Result<Option<String>, String> {
    let data = router_metadata(client, endpoint).await?;
    let id = data["routerId"]
        .as_str()
        .filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_owned);
    if expected.is_some_and(|expected| id.as_deref() != Some(expected)) {
        return Err(NETWORK_MISMATCH.into());
    }
    Ok(id)
}

async fn discovery_probe(client: &Client, origin: Url) -> Option<DiscoveredRouter> {
    let data = router_metadata(client, &origin).await.ok()?;
    Some(DiscoveredRouter {
        name: data["name"]
            .as_str()
            .unwrap_or("BJUT 路由器")
            .chars()
            .take(48)
            .collect(),
        address: origin.to_string(),
        version: data["version"]
            .as_str()
            .unwrap_or("--")
            .chars()
            .take(32)
            .collect(),
        secure: origin.scheme() == "https",
    })
}

#[tauri::command]
pub(crate) async fn discover_routers(
    last_address: Option<String>,
) -> Result<Vec<DiscoveredRouter>, String> {
    use futures_util::StreamExt;
    let gateways = tokio::task::spawn_blocking(crate::network_inventory::router_gateways)
        .await
        .map_err(|_| "无法读取当前局域网")?;
    let origins = discovery_origins(&gateways, last_address.as_deref());
    // Discovery reads only public metadata, without credentials/cookies. A
    // self-signed certificate may identify a candidate but is never trusted by
    // the separate authenticated RPC client until the user confirms its pin.
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(ReadOnlyCertificateProbe(PinnedCertificate {
        der: Vec::new(),
        algorithms: provider.signature_verification_algorithms,
    }));
    let tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| "无法准备路由器发现")?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .use_preconfigured_tls(tls)
        .connect_timeout(Duration::from_millis(800))
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|_| "无法准备路由器发现")?;
    let mut found: Vec<_> = futures_util::stream::iter(origins.into_iter().map(|origin| {
        let client = client.clone();
        async move { discovery_probe(&client, origin).await }
    }))
    .buffer_unordered(8)
    .filter_map(|result| async { result })
    .collect()
    .await;
    found.sort_by_key(|router| !router.secure);
    let mut hosts = std::collections::HashSet::new();
    found.retain(|router| {
        hosts.insert(
            router
                .address
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .to_string(),
        )
    });
    Ok(found)
}

fn valid_router_request(operation: &str, args: &Value) -> bool {
    let Some(args) = args.as_object() else {
        return false;
    };
    let service = [
        "enabled",
        "transport",
        "login_type",
        "compatibility",
        "strict_network",
        "expected_ssid",
        "expected_bssid",
        "interval",
        "timeout",
        "offline_confirmations",
        "max_backoff",
        "credential_cooldown",
        "post_login_delay",
        "online_url",
        "online_code",
        "managed_routes",
        "mwan3",
        "log_level",
        "log_target",
        "log_directory",
        "log_web",
        "log_max_kb",
        "pushplus_enabled",
        "pushplus_token",
        "pushplus_topic",
        "pushplus_online",
        "pushplus_failure",
        "pushplus_delay",
        "pushplus_interval",
        "billing_background",
        "traffic_monitor",
        "traffic_scope",
        "traffic_minute_hours",
        "traffic_hour_days",
        "traffic_day_days",
        "traffic_storage_mb",
        "prefer_remaining_flow",
        "billing_interval",
        "billing_alerts",
        "balance_threshold",
        "flow_threshold",
    ];
    let upstream = [
        "enabled",
        "network",
        "network6",
        "device",
        "device6",
        "source4",
        "source6",
        "transport",
        "login_type",
        "accounts",
        "expected_ssid",
        "expected_bssid",
    ];
    let allowed: &[&str] = match operation {
        "logs" | "settings" | "pushplus_test" | "pushplus_status" | "billing_cached"
        | "maintenance_info" => &[],
        "traffic" => &["start", "end", "resolution", "monthly"],
        "maintenance_start" => &["action", "prepared", "backup"],
        "maintenance_poll" => &["ticket"],
        "save_settings" => &service,
        "account_set" => &["account", "enabled"],
        "account_save" => &[
            "account",
            "username",
            "password",
            "enabled",
            "login_type",
            "max_sessions",
        ],
        "account_delete" => &["account"],
        "account_move" => &["account", "direction"],
        "uplink_save" => &["uplink", "payload"],
        "uplink_delete" => &["uplink"],
        "logout" | "switch_account" => &["uplink", "account"],
        "billing_start" => &["uplink", "account"],
        "billing_details" => &["uplink", "account", "module"],
        "billing_action" => &["ticket", "operation", "payload"],
        "billing_records" => &[
            "uplink",
            "account",
            "kind",
            "start_date",
            "end_date",
            "year",
            "page",
        ],
        "billing_poll" => &["ticket"],
        "billing_captcha" => &["ticket", "code", "refresh"],
        _ => return false,
    };
    if !args.iter().all(|(key, value)| {
        allowed.contains(&key.as_str())
            && match key.as_str() {
                "action" => matches!(
                    value.as_str(),
                    Some("check" | "prepare" | "backup" | "restore" | "upgrade" | "rollback")
                ),
                "prepared" => value
                    .as_str()
                    .is_some_and(|s| s.is_empty() || valid_token(s)),
                "backup" => value.as_str().is_some_and(|s| {
                    s.len() <= 80 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                }),
                "payload" if operation == "billing_action" => value.as_str().is_some_and(|text| {
                    valid_billing_payload(
                        args.get("operation").and_then(Value::as_str).unwrap_or(""),
                        text,
                    )
                }),
                "module" => matches!(
                    value.as_str(),
                    Some("services" | "devices" | "security" | "history" | "center")
                ),
                "payload" => value
                    .as_str()
                    .filter(|v| v.len() <= 32768)
                    .and_then(|v| serde_json::from_str::<Value>(v).ok())
                    .is_some_and(|v| {
                        v.as_object().is_some_and(|fields| {
                            !fields.is_empty()
                                && fields.iter().all(|(k, v)| {
                                    upstream.contains(&k.as_str())
                                        && v.as_str().is_some_and(|s| {
                                            s.len() <= 4096 && !s.chars().any(char::is_control)
                                        })
                                })
                        })
                    }),
                "start" | "end" => value
                    .as_i64()
                    .is_some_and(|v| (0..=41024448000).contains(&v)),
                "resolution" => matches!(value.as_i64(), Some(60 | 3600 | 86400)),
                "monthly" => value.is_boolean(),
                "traffic_scope" => matches!(value.as_str(), Some("local" | "all")),
                "refresh" => value.is_boolean(),
                "ticket" => value.as_str().is_some_and(valid_token),
                "account" | "uplink" => value
                    .as_str()
                    .is_some_and(|id| id.is_empty() || valid_section(id)),
                "code" => value
                    .as_str()
                    .is_some_and(|v| v.len() <= 12 && v.bytes().all(|b| b.is_ascii_alphanumeric())),
                "direction" => matches!(value.as_str(), Some("up" | "down")),
                "enabled"
                | "managed_routes"
                | "mwan3"
                | "strict_network"
                | "log_web"
                | "pushplus_enabled"
                | "pushplus_online"
                | "pushplus_failure"
                | "billing_background"
                | "billing_alerts"
                | "traffic_monitor"
                | "prefer_remaining_flow" => {
                    matches!(value.as_str(), Some("0" | "1"))
                }
                _ => value
                    .as_str()
                    .is_some_and(|v| v.len() <= 4096 && !v.chars().any(char::is_control)),
            }
    }) {
        return false;
    }
    match operation {
        "account_set" => args.contains_key("account") && args.contains_key("enabled"),
        "account_save" => {
            ["username", "password", "enabled", "login_type"]
                .iter()
                .all(|k| args.contains_key(*k))
                && args["username"]
                    .as_str()
                    .is_some_and(|v| !v.is_empty() && v.len() <= 256)
        }
        "account_delete" | "account_move" => {
            args.get("account")
                .and_then(Value::as_str)
                .is_some_and(|v| !v.is_empty())
                && (operation != "account_move" || args.contains_key("direction"))
        }
        "uplink_save" => args.contains_key("payload"),
        "uplink_delete" | "logout" | "switch_account" | "billing_start" | "billing_details"
        | "billing_records" => args
            .get("uplink")
            .and_then(Value::as_str)
            .is_some_and(|v| !v.is_empty()),
        "billing_poll" | "billing_captcha" | "maintenance_poll" => args.contains_key("ticket"),
        "maintenance_start" => args.contains_key("action"),
        "billing_action" => ["ticket", "operation", "payload"]
            .iter()
            .all(|key| args.contains_key(*key)),
        "save_settings" => !args.is_empty(),
        _ => true,
    }
}

fn valid_billing_payload(operation: &str, text: &str) -> bool {
    if text.len() > 16384 {
        return false;
    }
    let keys: &[&str] = match operation {
        "stopNow" | "reopenNow" | "cancelPackage" => &[],
        "schedulePackage" => &["packageId"],
        "setConsumeLimit" => &["consumeLimit"],
        "bindMac" | "unbindMac" => &["mac"],
        "disconnectSession" => &["sessionIndex"],
        "setMauth" => &["enabled"],
        "changePassword" => &["oldPassword", "newPassword"],
        "updateQuestions" => &[
            "oldPassword",
            "question1",
            "question2",
            "question3",
            "answer1",
            "answer2",
            "answer3",
        ],
        _ => return false,
    };
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .is_some_and(|object| {
            object.len() == keys.len()
                && keys.iter().all(|key| {
                    object
                        .get(*key)
                        .and_then(Value::as_str)
                        .is_some_and(|value| {
                            value.len() <= 4096 && !value.chars().any(char::is_control)
                        })
                })
        })
}

#[tauri::command]
pub(crate) async fn router_request(
    manager: tauri::State<'_, Manager>,
    operation: String,
    mut args: Value,
) -> Result<Value, String> {
    if !valid_router_request(&operation, &args) {
        return Err("路由器请求内容无效".into());
    }
    let remote = manager.0.lock().await.clone().ok_or("请先连接路由器")?;
    if operation.starts_with("billing_") {
        let mut tickets = remote.billing_tickets.lock().unwrap();
        tickets.retain(|_, created| created.elapsed() < Duration::from_secs(600));
        if matches!(
            operation.as_str(),
            "billing_start" | "billing_records" | "billing_details" | "billing_action"
        ) && tickets.len() >= 128
        {
            return Err("查询较多，请稍后重试".into());
        }
        if let Some(ticket) = args.get("ticket").and_then(Value::as_str) {
            if !tickets.contains_key(ticket) {
                return Err("计费查询已过期，请刷新".into());
            }
        }
        args["owner"] = Value::String(remote.token.clone());
    }
    let result = remote.call(&operation, args).await?;
    if matches!(
        operation.as_str(),
        "billing_start" | "billing_records" | "billing_details" | "billing_action"
    ) {
        let ticket = result
            .get("ticket")
            .and_then(Value::as_str)
            .filter(|token| valid_token(token))
            .ok_or("无法开始路由计费查询")?;
        remote
            .billing_tickets
            .lock()
            .unwrap()
            .insert(ticket.to_owned(), Instant::now());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn remembered_router_is_scoped_to_wifi_or_observed_wired_gateway() {
        let saved = RouterBinding {
            router_id: "a".repeat(32),
            ssid: "Campus router".into(),
            bssid: "00:11:22:33:44:55".into(),
            gateways: vec!["192.168.1.1".into()],
            transport: "wifi".into(),
        };
        let mut current = saved.clone();
        current.bssid = "00:11:22:33:44:66".into();
        assert!(
            same_network(&saved, &current),
            "roaming inside the same SSID still requires the same router ID"
        );
        current.ssid = "Different network".into();
        assert!(!same_network(&saved, &current));
        let mut wired = saved.clone();
        wired.ssid.clear();
        wired.bssid.clear();
        wired.transport = "ethernet".into();
        assert!(same_network(&wired, &current));
        current.gateways = vec!["10.0.0.1".into()];
        assert!(!same_network(&wired, &current));
        assert!(!same_network(
            &RouterBinding::default(),
            &RouterBinding::default()
        ));
        let old:SavedRouter=serde_json::from_value(json!({"address":"https://192.168.1.1","username":"root","password":"fixture","certificate":null,"allow_http":false})).unwrap();
        assert!(
            old.binding.is_none(),
            "legacy passwords must require manual pairing before reuse"
        );
    }

    #[test]
    fn wrong_router_identity_receives_no_password_or_session_token() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            for (valid_product, already_connected) in [(false, false), (true, false), (false, true), (true, true)] {
                let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let address=listener.local_addr().unwrap();
                let server=std::thread::spawn(move || {
                    let (mut stream,_)=listener.accept().unwrap();let mut bytes=Vec::new();let mut byte=[0];
                    while !bytes.ends_with(b"\r\n\r\n"){stream.read_exact(&mut byte).unwrap();bytes.push(byte[0]);}
                    let request=String::from_utf8(bytes).unwrap();assert!(request.starts_with("GET /.well-known/bjut-autologin.json "));assert!(!request.contains("fixture-password"));assert!(!request.contains(&"c".repeat(32)));
                    let body=json!({"product":if valid_product {"BJUT-Auto-Login-OpenWrt"} else {"Other router"},"apiVersion":1,"apiPath":"/ubus","routerId":"b".repeat(32)}).to_string();
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();drop(stream);
                    listener.set_nonblocking(true).unwrap();std::thread::sleep(Duration::from_millis(150));assert!(listener.accept().is_err(),"an authenticated request followed a failed identity check");
                });
                let profile=SavedRouter {address:format!("http://{address}"),username:"root".into(),password:"fixture-password".into(),certificate:None,allow_http:true,binding:Some(RouterBinding {router_id:"a".repeat(32),..Default::default()})};
                let rejected = if already_connected {
                    let remote = Remote {
                        client: router_client(None).unwrap(),
                        endpoint: Url::parse(&format!("http://{address}/ubus")).unwrap(),
                        token: "c".repeat(32),
                        router_id: Some("a".repeat(32)),
                        expires: std::sync::Mutex::new(Instant::now() + Duration::from_secs(60)),
                        lifetime: Duration::from_secs(60),
                        billing_tickets: std::sync::Mutex::new(std::collections::HashMap::new()),
                    };
                    remote.call("status", json!({})).await.is_err()
                } else { authenticate(&profile).await.is_err() };
                assert!(rejected);server.join().unwrap();
            }
        });
    }

    #[test]
    fn billing_actions_only_accept_exact_named_string_fields() {
        assert!(valid_billing_payload("stopNow", "{}"));
        assert!(valid_billing_payload(
            "changePassword",
            r#"{"oldPassword":"fixture-old","newPassword":"Fixture-123"}"#
        ));
        for (operation, payload) in [
            ("exec", "{}"),
            ("stopNow", r#"{"path":"/Self/unsafe"}"#),
            ("changePassword", r#"{"oldPassword":"x"}"#),
            ("setMauth", r#"{"enabled":true}"#),
            ("bindMac", r#"{"mac":"a\nb"}"#),
        ] {
            assert!(!valid_billing_payload(operation, payload));
        }
    }

    #[test]
    #[ignore = "requires an explicitly configured LAN router and interactive administrator password"]
    fn live_lan_router_read_only() {
        let address = std::env::var("BJUT_TEST_ROUTER_URL").expect("set BJUT_TEST_ROUTER_URL");
        let username = std::env::var("BJUT_TEST_ROUTER_USER").unwrap_or_else(|_| "root".into());
        let endpoint = endpoint_with_http(&address, true).unwrap();
        assert_eq!(
            endpoint.scheme(),
            "http",
            "this check uses trusted LAN HTTP"
        );
        eprintln!("Enter the existing router administrator password (not saved):");
        let mut password = String::new();
        std::io::stdin().read_line(&mut password).unwrap();
        let password = password.trim_end_matches(['\r', '\n']).to_owned();
        assert!(!password.is_empty());
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let client = router_client(None).unwrap();
            let mut origin = endpoint.clone();
            origin.set_path("/");
            assert!(discovery_probe(&client, origin).await.is_some());
            let login = rpc(
                &client,
                &endpoint,
                "00000000000000000000000000000000",
                "session",
                "login",
                json!({"username":username,"password":password,"timeout":60}),
            )
            .await
            .unwrap();
            let token = login["ubus_rpc_session"]
                .as_str()
                .filter(|value| valid_token(value))
                .unwrap()
                .to_owned();
            let remote = Remote {
                client,
                endpoint,
                token,
                router_id: None,
                expires: std::sync::Mutex::new(Instant::now() + Duration::from_secs(55)),
                lifetime: Duration::from_secs(55),
                billing_tickets: std::sync::Mutex::new(std::collections::HashMap::new()),
            };
            let snapshot = remote.snapshot().await;
            let settings = remote.call("settings", json!({})).await;
            let logs = remote.call("logs", json!({})).await;
            // Some stock ACLs do not expose session.destroy over HTTP; keep
            // their normal short expiry instead of expanding permissions.
            // No network, account or service configuration is changed.
            let logout = rpc(
                &remote.client,
                &remote.endpoint,
                &remote.token,
                "session",
                "destroy",
                json!({}),
            )
            .await;
            if let Err(error) = logout {
                assert!(error.contains("拒绝") || error.contains("权限"));
            }
            let snapshot = snapshot.unwrap();
            assert!(!snapshot.uplinks.is_empty());
            assert!(!snapshot.ports.is_empty());
            assert!(settings.is_ok() && logs.is_ok());
            eprintln!(
                "Live router read-only check passed: {} upstreams, {} ports",
                snapshot.uplinks.len(),
                snapshot.ports.len()
            );
        });
    }
    #[test]
    fn local_http_requires_explicit_opt_in_and_a_private_literal_address() {
        assert!(endpoint_with_http("http://192.168.1.1", false).is_err());
        for address in [
            "http://192.168.1.1",
            "http://10.0.0.1:8080",
            "http://[fd00::1]",
        ] {
            assert!(endpoint_with_http(address, true).is_ok(), "{address}");
        }
        for address in [
            "http://1.1.1.1",
            "http://router.lan",
            "http://198.18.0.1",
            "http://192.168.1.1/shell",
            "http://root:pass@192.168.1.1",
        ] {
            assert!(endpoint_with_http(address, true).is_err(), "{address}");
        }
    }

    #[test]
    fn discovery_is_bounded_to_observed_private_gateways_and_saved_local_address() {
        let urls = discovery_origins(
            &[
                "192.168.1.1".into(),
                "198.18.0.1".into(),
                "1.1.1.1".into(),
                "192.168.1.1".into(),
            ],
            Some("https://192.168.1.1:8443"),
        );
        assert_eq!(urls.len(), 3);
        assert!(urls
            .iter()
            .all(|url| url.host_str() == Some("192.168.1.1") && url.path() == "/"));
        assert!(discovery_origins(&[], Some("https://public.example")).is_empty());
    }

    #[test]
    fn collaboration_rejects_arbitrary_rpc_and_settings_requests() {
        assert!(valid_router_request(
            "account_save",
            &json!({"account":"account1","username":"25000001","password":"fixture password","enabled":"1","login_type":"lgn"})
        ));
        assert!(valid_router_request(
            "uplink_save",
            &json!({"uplink":"dorm_a","payload":r#"{"network":"wan14","network6":"wan16","source4":"10.126.19.139","accounts":"account1"}"#})
        ));
        assert!(valid_router_request(
            "billing_records",
            &json!({"uplink":"dorm_a","account":"current","kind":"usage","start_date":"2026-01-01","end_date":"2026-10-13","page":"1"})
        ));
        assert!(valid_router_request(
            "save_settings",
            &json!({"interval":"20","enabled":"1"})
        ));
        assert!(valid_router_request(
            "save_settings",
            &json!({"pushplus_enabled":"1","pushplus_token":"fixture-token","pushplus_delay":"60"})
        ));
        assert!(valid_router_request("pushplus_test", &json!({})));
        assert!(valid_router_request(
            "billing_details",
            &json!({"uplink":"wan1","account":"account1","module":"center"})
        ));
        assert!(valid_router_request(
            "billing_start",
            &json!({"uplink":"1","account":"current"})
        ));
        for (operation, args) in [
            ("exec", json!({"command":"reboot"})),
            (
                "billing_details",
                json!({"uplink":"wan1","account":"account1","module":"https://foreign.invalid"}),
            ),
            ("settings", json!({"path":"/etc/shadow"})),
            ("save_settings", json!({"password":"replacement"})),
            ("save_settings", json!({"pushplus_enabled":"true"})),
            ("pushplus_test", json!({"url":"https://other.example"})),
            (
                "uplink_save",
                json!({"payload":r#"{"network":"wan14","shell":"reboot"}"#}),
            ),
            ("uplink_save", json!({"payload":r#"{"enabled":true}"#})),
            ("account_delete", json!({"account":""})),
            (
                "account_save",
                json!({"username":"one","password":"secret","enabled":"1","login_type":"auto","file":"/etc/shadow"}),
            ),
            ("account_set", json!({"account":"x;reboot","enabled":"0"})),
            (
                "billing_poll",
                json!({"ticket":"1".repeat(32),"owner":"2".repeat(32)}),
            ),
            (
                "billing_captcha",
                json!({"ticket":"1".repeat(32),"code":"../file","refresh":false}),
            ),
        ] {
            assert!(!valid_router_request(operation, &args), "{operation}");
        }
    }

    #[test]
    fn discovery_validates_public_metadata_without_credentials_and_refuses_redirects() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            for case in 0..4 {
                let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                let address=listener.local_addr().unwrap();
                let worker=std::thread::spawn(move || {
                    let (mut stream,_)=listener.accept().unwrap();
                    let mut request=Vec::new(); let mut byte=[0];
                    while !request.ends_with(b"\r\n\r\n") {stream.read_exact(&mut byte).unwrap();request.push(byte[0]);}
                    let request=String::from_utf8(request).unwrap();
                    assert!(request.starts_with("GET /.well-known/bjut-autologin.json "));
                    assert!(!request.to_ascii_lowercase().contains("authorization:"));
                    assert!(!request.to_ascii_lowercase().contains("cookie:"));
                    if case==1 {
                        stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/other\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                    } else {
                        let body=if case==2 {"x".repeat(8193)} else {
                            json!({"product":if case==3 {"unrelated"} else {"BJUT-Auto-Login-OpenWrt"},"apiVersion":1,"apiPath":"/ubus","version":"0.1.6","name":"Fixture router"}).to_string()
                        };
                        write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
                    }
                });
                let result=discovery_probe(&router_client(None).unwrap(),Url::parse(&format!("http://{address}/")).unwrap()).await;
                assert_eq!(result.is_some(),case==0);
                worker.join().unwrap();
            }
        });
    }
    #[test]
    fn router_endpoint_preserves_origin_and_requires_tls() {
        assert_eq!(
            endpoint("https://192.168.1.1:8443/").unwrap().as_str(),
            "https://192.168.1.1:8443/ubus"
        );
        for url in [
            "http://192.168.1.1",
            "https://root:pass@192.168.1.1",
            "https://192.168.1.1/?token=x",
            "https://192.168.1.1/shell",
            "file:///etc/passwd",
        ] {
            assert!(endpoint(url).is_err(), "{url}");
        }
    }
    #[test]
    fn only_named_uplink_and_account_ids_are_allowed() {
        assert!(valid_section("dorm_a"));
        for id in ["", "../main", "main;reboot", "wan14 $(reboot)"] {
            assert!(!valid_section(id));
        }
        assert!(!valid_token("00000000000000000000000000000000"));
        assert!(valid_token("1234567890abcdef1234567890abcdef"));
    }

    #[test]
    fn rpc_validates_request_identity_and_refuses_redirects() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
        for redirect in [false, true] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let worker = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut data = Vec::new();
                let mut byte = [0];
                while !data.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    data.push(byte[0]);
                }
                let headers = String::from_utf8(data).unwrap();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|n| n.trim().parse().unwrap())
                    })
                    .unwrap();
                let mut body = vec![0; length];
                stream.read_exact(&mut body).unwrap();
                let value: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(value["params"][1], "bjut-autologin");
                if redirect {
                    stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                } else {
                    let response = json!({"jsonrpc":"2.0","id":value["id"].as_u64().unwrap()+1,"result":[0,{}]}).to_string();
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        response.len(),
                        response
                    )
                    .unwrap();
                }
            });
            let client = router_client(None).unwrap();
            let error = rpc(
                &client,
                &Url::parse(&format!("http://{address}/ubus")).unwrap(),
                "1".repeat(32).as_str(),
                "bjut-autologin",
                "status",
                json!({}),
            )
            .await
            .unwrap_err();
            assert!(error.contains(if redirect { "302" } else { "拒绝" }));
            worker.join().unwrap();
        }
        });
    }

    #[test]
    fn session_logout_accepts_no_payload_but_status_still_requires_data() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            for (object, method, accepted) in [
                ("session", "destroy", true),
                ("bjut-autologin", "status", false),
            ] {
                let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                let address = listener.local_addr().unwrap();
                let worker = std::thread::spawn(move || {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut headers = Vec::new();
                    let mut byte = [0];
                    while !headers.ends_with(b"\r\n\r\n") {
                        stream.read_exact(&mut byte).unwrap();
                        headers.push(byte[0]);
                    }
                    let length: usize = String::from_utf8(headers)
                        .unwrap()
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|n| n.trim().parse().unwrap())
                        })
                        .unwrap();
                    let mut body = vec![0; length];
                    stream.read_exact(&mut body).unwrap();
                    let request: Value = serde_json::from_slice(&body).unwrap();
                    assert_eq!(request["params"][3], json!({}));
                    let response =
                        json!({"jsonrpc":"2.0","id":request["id"],"result":[0]}).to_string();
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        response.len(),
                        response
                    )
                    .unwrap();
                });
                let result = rpc(
                    &router_client(None).unwrap(),
                    &Url::parse(&format!("http://{address}/ubus")).unwrap(),
                    &"1".repeat(32),
                    object,
                    method,
                    json!({}),
                )
                .await;
                assert_eq!(result.is_ok(), accepted);
                worker.join().unwrap();
            }
        });
    }

    #[test]
    fn explicitly_pinned_router_certificate_still_requires_tls_key_proof() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            struct Server(std::process::Child, std::path::PathBuf);
            impl Drop for Server {
                fn drop(&mut self) {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                    let _ = std::fs::remove_dir_all(&self.1);
                }
            }
            let dir = std::env::temp_dir().join(format!(
                "bjut-router-tls-{}-{}",
                std::process::id(),
                REQUEST_ID.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&dir).unwrap();
            let cert = dir.join("router.pem");
            let key = dir.join("router.key");
            let generated = std::process::Command::new("openssl")
                .args([
                    "req",
                    "-x509",
                    "-newkey",
                    "rsa:2048",
                    "-nodes",
                    "-subj",
                    "/CN=fixture.router.invalid",
                    "-days",
                    "1",
                    "-out",
                ])
                .arg(&cert)
                .arg("-keyout")
                .arg(&key)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(generated.success());
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            drop(listener);
            let child = std::process::Command::new("openssl")
                .args([
                    "s_server",
                    "-accept",
                    &address.to_string(),
                    "-www",
                    "-quiet",
                    "-cert",
                ])
                .arg(&cert)
                .arg("-key")
                .arg(&key)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let _server = Server(child, dir);
            for _ in 0..40 {
                if std::net::TcpStream::connect(address).is_ok() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            let url = format!("https://{address}/");
            let pem = std::fs::read_to_string(cert).unwrap();
            assert!(router_client(Some(&pem))
                .unwrap()
                .get(&url)
                .send()
                .await
                .unwrap()
                .status()
                .is_success());
            let untrusted = router_client(None)
                .unwrap()
                .get(&url)
                .send()
                .await
                .unwrap_err();
            assert!(
                connection_error(&untrusted).starts_with("ROUTER_CERT_UNTRUSTED:"),
                "{untrusted:?}"
            );
            let received = probe_router_certificate(url.clone()).await.unwrap();
            let accepted = router_client(Some(&received.pem))
                .unwrap()
                .get(&url)
                .send()
                .await
                .unwrap();
            assert!(accepted.status().is_success());
            assert_eq!(received.fingerprint.split(':').count(), 32);
            assert_eq!(received.pem.trim(), pem.trim());
            let other = include_str!("portal_auth/fixtures/dorm-gateway-2024.pem");
            assert!(router_client(Some(other))
                .unwrap()
                .get(&url)
                .send()
                .await
                .is_err());
        });
    }

    #[test]
    fn closed_https_port_reports_service_problem_before_certificate_trust() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("https://{}/", listener.local_addr().unwrap());
            drop(listener);
            let error = router_client(None)
                .unwrap()
                .get(&url)
                .send()
                .await
                .unwrap_err();
            assert!(connection_error(&error).contains("未接受 HTTPS"));
            assert!(probe_router_certificate(url)
                .await
                .unwrap_err()
                .contains("未接受 HTTPS"));
        });
    }
}
