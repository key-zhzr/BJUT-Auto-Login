//! The App talks only to the named, authenticated rpcd methods shipped by the
//! router package. Router credentials/session tokens never enter persistent UI storage.
use reqwest::{Client, Url};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const MAX_RESPONSE: usize = 256 * 1024;
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

#[derive(Default)]
pub(crate) struct Manager(Mutex<Option<Remote>>);

struct Remote {
    client: Client,
    endpoint: Url,
    token: String,
    expires: std::sync::Mutex<Instant>,
    lifetime: Duration,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    version: String,
    uplinks: Vec<Value>,
    accounts: Vec<Value>,
    management_url: String,
}

fn endpoint(value: &str) -> Result<Url, String> {
    let mut url = Url::parse(value.trim()).map_err(|_| "请填写完整的路由器 HTTPS 地址")?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"));
    if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/" | "/ubus" | "/ubus/")
    {
        return Err("请使用路由器的 HTTPS 地址，例如 https://192.168.1.1".into());
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
        .map_err(|error| {
            if error.is_timeout() {
                "路由器连接超时，请检查地址与网络"
            } else {
                "无法连接路由器，请检查 HTTPS 地址和证书"
            }
            .to_string()
        })?;
    if !response.status().is_success() {
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
        let status = self.call("status", json!({})).await?;
        if status.get("apiVersion").and_then(Value::as_u64) != Some(1) {
            return Err("路由端协作接口不兼容，请更新两端至 0.1.6".into());
        }
        let accounts = self.call("accounts", json!({})).await?;
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
        })
    }
}

#[tauri::command]
pub(crate) async fn connect_router(
    manager: tauri::State<'_, Manager>,
    address: String,
    username: String,
    password: String,
    certificate: Option<String>,
) -> Result<Snapshot, String> {
    let endpoint = endpoint(&address)?;
    if username.trim().is_empty()
        || username.len() > 128
        || password.is_empty()
        || password.len() > 1024
    {
        return Err("请填写路由器管理账号与密码".into());
    }
    let client = router_client(certificate.as_deref())?;
    let login = Box::pin(rpc(
        &client,
        &endpoint,
        "00000000000000000000000000000000",
        "session",
        "login",
        json!({"username":username.trim(),"password":password,"timeout":300}),
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
        expires: std::sync::Mutex::new(
            Instant::now() + Duration::from_secs(timeout.saturating_sub(5)),
        ),
        lifetime: Duration::from_secs(timeout.saturating_sub(5)),
    };
    let snapshot = Box::pin(remote.snapshot()).await?;
    *manager.0.lock().await = Some(remote);
    Ok(snapshot)
}

#[tauri::command]
pub(crate) async fn get_router_status(
    manager: tauri::State<'_, Manager>,
) -> Result<Snapshot, String> {
    let guard = manager.0.lock().await;
    let remote = guard.as_ref().ok_or("请先连接路由器")?;
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
    let guard = manager.0.lock().await;
    let remote = guard.as_ref().ok_or("请先连接路由器")?;
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
    let guard = manager.0.lock().await;
    let remote = guard.as_ref().ok_or("请先连接路由器")?;
    Box::pin(remote.call("sync_accounts", json!({"payload":serde_json::to_string(&json!({"accounts":accounts})).map_err(|_| "账号内容无法读取")?}))).await?;
    Ok("选定账号已同步到路由器".into())
}

#[tauri::command]
pub(crate) async fn disconnect_router(manager: tauri::State<'_, Manager>) -> Result<(), String> {
    if let Some(remote) = manager.0.lock().await.take() {
        let _ = Box::pin(rpc(
            &remote.client,
            &remote.endpoint,
            &remote.token,
            "session",
            "destroy",
            json!({"ubus_rpc_session":remote.token}),
        ))
        .await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
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
            assert!(router_client(None).unwrap().get(&url).send().await.is_err());
            let other = include_str!("portal_auth/fixtures/dorm-gateway-2024.pem");
            assert!(router_client(Some(other))
                .unwrap()
                .get(&url)
                .send()
                .await
                .is_err());
        });
    }
}
