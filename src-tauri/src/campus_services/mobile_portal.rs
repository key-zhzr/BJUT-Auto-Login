//! App-session bridge verified against the supplied 日新工大 2.8.8 APK.
//! Native API requests use an RSA form envelope; WebView navigation uses cookies.
//! The two must share a cookie jar. No captured tickets or device IDs are reused.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use openssl::{
    pkey::PKey,
    rsa::{Padding, Rsa},
};

#[path = "mobile_portal_codec_keys.rs"]
mod codec_keys;

pub(super) const CALLBACK: &str = "/bjutapp/wap/app-login/local-login";
pub(super) const ENTRY: &str = "https://itsapp.bjut.edu.cn/a_bjut/api/sso/index?redirect=/bjutapp/wap/app-login/local-login&from=wap";
pub(super) const WEB_USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 13; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/126.0.0.0 Mobile Safari/537.36 ZhilinEai ZhilinBjutApp/2.8.8";
const API_USER_AGENT: &str = "ZhilinEai ZhilinBjutApp/2.8.8 BJUT-AL Android 13";
const MAX_WIRE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct SessionState {
    device_id: String,
    ticket: String,
}
impl std::fmt::Debug for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionState")
            .field("authenticated", &!self.ticket.is_empty())
            .finish()
    }
}
impl SessionState {
    pub(super) fn valid(&self) -> bool {
        self.device_id.len() == 36
            && self
                .device_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
            && self.ticket.len() <= 8192
            && !self.ticket.chars().any(char::is_control)
    }
    fn new() -> Self {
        use aes_gcm::aead::{rand_core::RngCore, OsRng};
        let mut bytes = [0u8; 16];
        OsRng.fill_bytes(&mut bytes);
        bytes[6] = (bytes[6] & 15) | 64;
        bytes[8] = (bytes[8] & 63) | 128;
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        Self {
            device_id: format!(
                "{}-{}-{}-{}-{}",
                &hex[..8],
                &hex[8..12],
                &hex[12..16],
                &hex[16..20],
                &hex[20..]
            ),
            ticket: String::new(),
        }
    }
}

pub(super) fn is_callback(url: &Url) -> bool {
    url.host_str() == Some(ITS_HOST)
        && url.path() == CALLBACK
        && url.query().is_none()
        && url.fragment().is_none()
}

pub(super) async fn prepare(
    session: &mut CampusSession,
    account: &str,
    password: &str,
) -> Result<(), CampusServiceError> {
    let state = session
        .app_session
        .get_or_insert_with(SessionState::new)
        .clone();
    // An anonymous check also establishes eai-sess/UUkey (verified against the
    // service). No install-stat telemetry or hardware identifiers are needed.
    let checked = api_post(
        session,
        "/bjutapp/wap/app-login/check",
        json!({"ticket":state.ticket}),
    )
    .await?;
    if successful(&checked) && !state.ticket.is_empty() {
        return Ok(());
    }
    if !successful(&checked) && checked.get("e").and_then(Value::as_i64) != Some(10013) {
        return ensure_successful(&checked, "校验校园一卡通会话");
    }
    // Only the explicit expired-session code renews CAS. Transport/codec errors
    // above never trigger another password submission.
    session.app_session.as_mut().unwrap().ticket.clear();
    let entry = parse_url(ENTRY)?;
    let (mut url, html) = get_follow_text(session, entry, &[ITS_HOST, CAS_HOST], None).await?;
    if url.host_str() == Some(CAS_HOST) && url.path() == "/login" {
        let (action, fields) =
            cas_login_form(&html, &url, account, password, CasTarget::AppPortal)?;
        let response = send_form(session, action.clone(), &fields, CAS_HOST, url.as_str()).await?;
        if matches!(response.status().as_u16(), 307 | 308) {
            return Err(CampusServiceError::protocol(
                "CAS 要求重复提交密码，已安全中止",
            ));
        }
        if !response.status().is_redirection() {
            return Err(CampusServiceError::rejected(
                "统一认证未完成，请核对密码或在学校统一认证页完成验证后重试",
            ));
        }
        let next = redirect_target(&action, &response, &[CAS_HOST, ITS_HOST])?;
        let _ = response.bytes().await;
        (url, _) = get_follow_text(session, next, &[CAS_HOST, ITS_HOST], Some(&action)).await?;
    }
    if !is_callback(&url) {
        return Err(CampusServiceError::rejected(
            "校园一卡通登录未完成，请稍后重试",
        ));
    }
    // local-login is a WebView callback intercepted by the official app, not
    // a page to load (requesting it returns a 404 page). Cookies already exist.
    let logged_in = api_post(
        session,
        "/bjutapp/wap/app-login/login",
        json!({
            "imei":state.device_id, "sid":state.device_id, "mobile_type":"android"
        }),
    )
    .await?;
    ensure_successful(&logged_in, "登录校园一卡通")?;
    let ticket = logged_in
        .pointer("/d/login_ticket")
        .and_then(Value::as_str)
        .filter(|ticket| {
            !ticket.is_empty() && ticket.len() <= 8192 && !ticket.chars().any(char::is_control)
        })
        .ok_or_else(|| CampusServiceError::protocol("校园一卡通登录缺少会话凭据"))?
        .to_owned();
    session.app_session.as_mut().unwrap().ticket = ticket.clone();
    let checked = api_post(
        session,
        "/bjutapp/wap/app-login/check",
        json!({"ticket":ticket}),
    )
    .await?;
    ensure_successful(&checked, "校验校园一卡通会话")
}

fn successful(value: &Value) -> bool {
    value
        .get("e")
        .is_some_and(|code| code.as_i64() == Some(0) || code.as_str() == Some("0"))
}
fn ensure_successful(value: &Value, operation: &str) -> Result<(), CampusServiceError> {
    if successful(value) {
        Ok(())
    } else {
        Err(CampusServiceError::rejected(format!(
            "{operation}失败：{}",
            response_message(value)
        )))
    }
}

fn encode(value: &Value) -> Result<String, CampusServiceError> {
    let public = Rsa::public_key_from_pem(codec_keys::REQUEST_PUBLIC_KEY)
        .map_err(|_| CampusServiceError::protocol("无法准备校园一卡通请求"))?;
    encrypt_with_key(
        &public,
        &serde_json::to_vec(value)
            .map_err(|_| CampusServiceError::protocol("校园一卡通请求内容无效"))?,
    )
}

fn encrypt_with_key(
    key: &Rsa<openssl::pkey::Public>,
    input: &[u8],
) -> Result<String, CampusServiceError> {
    let size = key.size() as usize;
    let mut output = Vec::new();
    let mut block = vec![0; size];
    for part in input.chunks(size - 11) {
        let used = key
            .public_encrypt(part, &mut block, Padding::PKCS1)
            .map_err(|_| CampusServiceError::protocol("校园一卡通请求编码失败"))?;
        output.extend_from_slice(&block[..used]);
    }
    Ok(STANDARD.encode(output))
}

fn decode(text: &str) -> Result<Value, CampusServiceError> {
    if text.len() > MAX_WIRE_BYTES {
        return Err(CampusServiceError::protocol("校园一卡通响应过大"));
    }
    // The official network library accepts a plain error object as well as an
    // encrypted response. Never treat malformed ciphertext as a successful login.
    let text = text.trim();
    let parsed = serde_json::from_str::<Value>(text).ok();
    if let Some(value @ Value::Object(_)) = parsed {
        return Ok(value);
    }
    let encrypted = parsed.as_ref().and_then(Value::as_str).unwrap_or(text);
    let key = PKey::private_key_from_pem(codec_keys::RESPONSE_DECODER)
        .and_then(|key| key.rsa())
        .map_err(|_| CampusServiceError::protocol("无法读取校园一卡通响应"))?;
    decrypt_with_key(&key, encrypted)
}

fn decrypt_with_key(
    key: &Rsa<openssl::pkey::Private>,
    encrypted: &str,
) -> Result<Value, CampusServiceError> {
    let failure =
        || CampusServiceError::protocol("校园一卡通会话响应无法解析，请更新应用或稍后重试");
    let bytes = STANDARD.decode(encrypted).map_err(|_| failure())?;
    let size = key.size() as usize;
    if bytes.is_empty() || bytes.len() % size != 0 {
        return Err(failure());
    }
    let mut output = Vec::new();
    let mut block = vec![0; size];
    for part in bytes.chunks_exact(size) {
        let used = key
            .private_decrypt(part, &mut block, Padding::PKCS1)
            .map_err(|_| failure())?;
        output.extend_from_slice(&block[..used]);
    }
    serde_json::from_slice(&output).map_err(|_| failure())
}

async fn api_post(
    session: &mut CampusSession,
    path: &str,
    body: Value,
) -> Result<Value, CampusServiceError> {
    if !matches!(
        path,
        "/bjutapp/wap/app-login/login" | "/bjutapp/wap/app-login/check"
    ) {
        return Err(CampusServiceError::protocol("校园一卡通接口不受支持"));
    }
    let url = parse_url(&format!("https://{ITS_HOST}{path}"))?;
    let ticket = session
        .app_session
        .as_ref()
        .map(|state| state.ticket.as_str())
        .unwrap_or("");
    let authorization =
        encode(&json!({"timestamp":unix_timestamp().to_string(), "ticket":ticket}))?;
    let content = encode(&body)?;
    let mut request = session
        .client
        .post(url.clone())
        .header(USER_AGENT, API_USER_AGENT)
        .header(ACCEPT_LANGUAGE, "zh-Hans-CN;q=1, en-CN;q=0.9")
        .header("from-eai", "1")
        .header("authorization-str", authorization)
        .form(&[("content", content)]);
    if let Some(cookie) = session.cookies.header(&url) {
        request = request.header(COOKIE, cookie);
    }
    let response = request
        .send()
        .await
        .map_err(|error| CampusServiceError::network("建立校园一卡通会话", error))?;
    session.cookies.absorb(&url, response.headers());
    if response.status().is_redirection() {
        return Err(CampusServiceError::rejected(
            "校园一卡通会话已失效，请重新核对充值信息",
        ));
    }
    if !response.status().is_success() {
        return Err(CampusServiceError::rejected(format!(
            "校园一卡通暂不可用（HTTP {}）",
            response.status().as_u16()
        )));
    }
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(part) = stream.next().await {
        let part =
            part.map_err(|error| CampusServiceError::network("读取校园一卡通会话", error))?;
        if bytes.len() + part.len() > MAX_WIRE_BYTES {
            return Err(CampusServiceError::protocol("校园一卡通响应过大"));
        }
        bytes.extend_from_slice(&part);
    }
    decode(
        std::str::from_utf8(&bytes)
            .map_err(|_| CampusServiceError::protocol("校园一卡通响应编码无效"))?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_session_bridge_preserves_cookies_and_stops_at_callback() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
        use openssl::{
            asn1::Asn1Time,
            hash::MessageDigest,
            ssl::{SslAcceptor, SslMethod},
            x509::{
                extension::{BasicConstraints, SubjectAlternativeName},
                X509NameBuilder, X509,
            },
        };
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        // Local TLS peers keep production host, certificate, and redirect checks
        // enabled. Credentials/tickets here are synthetic and never leave localhost.
        let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text("CN", ITS_HOST).unwrap();
        let name = name.build();
        let mut cert = X509::builder().unwrap();
            cert.set_version(2).unwrap();
            cert.set_serial_number(&openssl::bn::BigNum::from_u32(1).unwrap().to_asn1_integer().unwrap()).unwrap();
        cert.set_subject_name(&name).unwrap();
        cert.set_issuer_name(&name).unwrap();
        cert.set_pubkey(&key).unwrap();
        cert.set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        cert.set_not_after(&Asn1Time::days_from_now(1).unwrap())
            .unwrap();
        cert.append_extension(BasicConstraints::new().critical().build().unwrap())
            .unwrap();
        cert.append_extension(
            SubjectAlternativeName::new()
                .dns(ITS_HOST)
                .dns(CAS_HOST)
                .build(&cert.x509v3_context(None, None))
                .unwrap(),
        )
        .unwrap();
        cert.sign(&key, MessageDigest::sha256()).unwrap();
        let cert = cert.build();
        let mut acceptor = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
        acceptor.set_certificate(&cert).unwrap();
        acceptor.set_private_key(&key).unwrap();
        let acceptor = acceptor.build();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
                .use_rustls_tls()
            .add_root_certificate(reqwest::Certificate::from_pem(&cert.to_pem().unwrap()).unwrap())
            .resolve(ITS_HOST, addr)
            .resolve(CAS_HOST, addr)
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let server = std::thread::spawn(move || {
            let private = PKey::private_key_from_pem(codec_keys::RESPONSE_DECODER)
                .unwrap()
                .rsa()
                .unwrap();
            let public = Rsa::public_key_from_pem(&private.public_key_to_pem().unwrap()).unwrap();
            let mut cas = Url::parse("https://cas.bjut.edu.cn/login").unwrap();
            cas.query_pairs_mut()
                .append_pair("noAutoRedirect", "1")
                .append_pair("service", ENTRY);
            let started = Instant::now();
            for step in 0..8 {
                let tcp = loop {
                    match listener.accept() {
                        Ok((tcp, _)) => break tcp,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && started.elapsed() < Duration::from_secs(10) =>
                        {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        Err(error) => panic!("bridge stopped before step {step}: {error}"),
                    }
                };
                tcp.set_nonblocking(false).unwrap();
                tcp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut stream = acceptor.accept(tcp).unwrap();
                let mut bytes = Vec::new();
                let (header_end, length) = loop {
                    let mut buffer = [0; 2048];
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]);
                        let length = header
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        break (end + 4, length);
                    }
                };
                while bytes.len() < header_end + length {
                    let mut buffer = [0; 2048];
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                }
                let request = String::from_utf8(bytes).unwrap();
                let header = request[..header_end].to_ascii_lowercase();
                let first = request.lines().next().unwrap();
                assert!(!first.contains(CALLBACK) || first.contains("redirect="));
                let (status, headers, body) = match step {
                    0 => {
                        assert!(first.starts_with("POST /bjutapp/wap/app-login/check "));
                        assert!(
                            header.contains("from-eai: 1") && header.contains("authorization-str:")
                        );
                        assert!(request[header_end..].starts_with("content="));
                        (
                            200,
                            "Set-Cookie: UUkey=visitor; Path=/; Secure; HttpOnly\r\n".into(),
                            encrypt_with_key(&public, br#"{"e":10013,"m":"expired"}"#).unwrap(),
                        )
                    }
                    1 => {
                        assert!(header.contains("uukey=visitor"));
                        (302, format!("Location: {cas}\r\n"), String::new())
                    }
                    2 => {
                        assert!(first.starts_with("GET /login?"));
                        assert!(!header.contains("uukey"));
                        (200, "Set-Cookie: CASTGC=cas-only; Path=/; Secure\r\n".into(), "<form action=''><input name='execution' value='test-execution'><input name='username'><input name='password'></form>".into())
                    }
                    3 => {
                        assert!(first.starts_with("POST /login?"));
                        assert!(header.contains("castgc=cas-only"));
                        let fields: BTreeMap<_, _> = url_form(&request[header_end..]);
                        assert_eq!(fields["username"], "fixture-account");
                        assert_eq!(fields["password"], "fixture-password");
                        (
                            302,
                            format!("Location: {ENTRY}&ticket=cas-ticket\r\n"),
                            String::new(),
                        )
                    }
                    4 => {
                        assert!(!header.contains("castgc"));
                        (200, String::new(), "window.location.href='?redirect=%2Fbjutapp%2Fwap%2Fapp-login%2Flocal-login&from=wap&ticket=cas-ticket&12345678dictkey='+md5('192.0.2.4');".into())
                    }
                    5 => {
                        assert!(first.contains("12345678dictkey="));
                        (302, format!("Location: {CALLBACK}\r\nSet-Cookie: eai-sess=authenticated; Path=/; Secure; HttpOnly\r\n"), String::new())
                    }
                    6 => {
                        assert!(first.starts_with("POST /bjutapp/wap/app-login/login "));
                        assert!(header.contains("eai-sess=authenticated"));
                        (
                            200,
                            String::new(),
                            encrypt_with_key(
                                &public,
                                br#"{"e":0,"d":{"login_ticket":"new-fixture-ticket"}}"#,
                            )
                            .unwrap(),
                        )
                    }
                    _ => {
                        assert!(first.starts_with("POST /bjutapp/wap/app-login/check "));
                        (
                            200,
                            String::new(),
                            encrypt_with_key(&public, br#"{"e":0}"#).unwrap(),
                        )
                    }
                };
                write!(stream, "HTTP/1.1 {status} OK\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                stream.flush().unwrap();
            }
        });
        let mut session = CampusSession {
            client,
            cookies: CookieJar::default(),
            app_session: None,
        };
        let result = prepare(&mut session, "fixture-account", "fixture-password").await;
        server.join().unwrap();
        result.unwrap();
        assert_eq!(
            session.app_session.as_ref().unwrap().ticket,
            "new-fixture-ticket"
        );
        let saved = session.persisted("fixture-account");
        let restored = CampusSession::new("fixture-account", Some(saved.clone())).unwrap();
        assert_eq!(restored.app_session, session.app_session);
        assert!(CampusSession::new("different-account", Some(saved))
            .unwrap()
            .app_session
            .is_none());
        });
    }

    fn url_form(body: &str) -> BTreeMap<String, String> {
        Url::parse(&format!("https://example.test/?{body}"))
            .unwrap()
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect()
    }
    #[test]
    fn wire_codec_handles_multiple_blocks_utf8_and_rejects_corruption() {
        let private = PKey::private_key_from_pem(codec_keys::RESPONSE_DECODER)
            .unwrap()
            .rsa()
            .unwrap();
        let public = Rsa::public_key_from_pem(&private.public_key_to_pem().unwrap()).unwrap();
        let original = json!({"e":0, "d":{"login_ticket":"fixture"}, "m":"校园一卡通".repeat(80)});
        let encrypted = encrypt_with_key(&public, &serde_json::to_vec(&original).unwrap()).unwrap();
        assert_eq!(decode(&encrypted).unwrap(), original);
        assert_eq!(
            decode(&serde_json::to_string(&encrypted).unwrap()).unwrap(),
            original
        );
        assert!(decode(&encrypted[..encrypted.len() - 4]).is_err());
        assert!(decode("<html>资源受限</html>").is_err());
        assert_eq!(decode(r#"{"e":1,"m":"请重新登录"}"#).unwrap()["e"], 1);
        assert!(!successful(&json!({"d":{}})));
        assert!(encode(&json!({"timestamp":"1800000000","ticket":"fixture"})).is_ok());
    }
    #[test]
    fn callback_is_exact_and_device_identity_is_random() {
        assert!(is_callback(
            &Url::parse(&format!("https://{ITS_HOST}{CALLBACK}")).unwrap()
        ));
        for suffix in ["?ticket=unexpected", "#fragment", "/elsewhere"] {
            assert!(!is_callback(
                &Url::parse(&format!("https://{ITS_HOST}{CALLBACK}{suffix}")).unwrap()
            ));
        }
        let a = SessionState::new();
        assert!(a.valid());
        assert_ne!(a.device_id, SessionState::new().device_id);
        assert!(!format!("{a:?}").contains(&a.device_id));
    }
}
