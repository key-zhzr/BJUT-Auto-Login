//! School WebVPN transport and interactive CAS/SMS login. Sessions/execution
//! values remain in memory; no HAR cookies or tokens are replayed.
use super::*;

pub(crate) const ORIGIN: &str = "https://webvpn.bjut.edu.cn";
pub(crate) const HOST: &str = "webvpn.bjut.edu.cn";
pub(crate) const BILLING_PREFIX: &str =
    "/https/77726476706e69737468656265737421faf152992b362652741d9de29d51367b6c5c";
const CAS_PREFIX: &str = "/https/77726476706e69737468656265737421f3f652d2253a7d44300d8db9d6562d";
const PORTAL_PREFIX: &str =
    "/https/77726476706e69737468656265737421e7f2438a373e2652741d9de29d51367b1d41";

#[derive(Clone)]
pub(crate) struct Session {
    pub(crate) id: String,
    pub(crate) client: Client,
    pub(crate) cookies: CookieJar,
}
struct Pending {
    session: Session,
    account: String,
    action: Url,
    execution: String,
    id: String,
    expires: Instant,
    last_send: Option<Instant>,
    last_submit: Option<Instant>,
}
struct ChallengeProgress {
    id: String,
    expires: Instant,
    last_send: Option<Instant>,
    last_submit: Option<Instant>,
    resend: bool,
}
#[derive(Default)]
pub(crate) struct Manager {
    credential_stamp: Option<[u8; 32]>,
    ready: Option<(String, Session, Instant)>,
    pending: Option<Pending>,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginStatus {
    stage: &'static str,
    challenge_id: Option<String>,
    message: String,
}

impl Manager {
    pub(crate) fn bind_credentials(&mut self, stamp: [u8; 32]) {
        if self.credential_stamp != Some(stamp) {
            self.clear();
            self.credential_stamp = Some(stamp);
        }
    }
    pub(crate) fn clear(&mut self) {
        self.ready = None;
        self.pending = None;
    }
    pub(crate) fn session(&self) -> Result<Session, String> {
        self.ready
            .as_ref()
            .filter(|(_, _, time)| time.elapsed() < Duration::from_secs(8 * 3600))
            .map(|(_, session, _)| session.clone())
            .ok_or_else(|| "请先完成校外 WebVPN 登录".into())
    }
    pub(crate) fn cancel(&mut self, id: &str) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id == id)
        {
            self.pending = None;
        }
    }
    pub(crate) async fn begin(
        &mut self,
        account: &str,
        password: &str,
    ) -> Result<LoginStatus, String> {
        self.pending = None;
        let mut session = if let Some((_, session, _)) =
            self.ready.as_ref().filter(|(user, _, time)| {
                user == account && time.elapsed() < Duration::from_secs(8 * 3600)
            }) {
            session.clone()
        } else {
            Session {
                id: challenge_id(),
                client: Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(8))
                    .timeout(Duration::from_secs(20))
                    .user_agent(BILLING_USER_AGENT)
                    .use_rustls_tls()
                    .build()
                    .map_err(|_| "无法准备校外连接")?,
                cookies: CookieJar::default(),
            }
        };
        // Establish a fresh WebVPN visitor ticket before entering proxied CAS.
        let _ = get(&mut session, Url::parse(ORIGIN).unwrap()).await?;
        if verified_account(&mut session, account).await {
            return Ok(self.accept(account, session));
        }
        let (url, html) = get(&mut session, login_url()).await?;
        if !is_cas_login(&url) {
            if verified_account(&mut session, account).await {
                return Ok(self.accept(account, session));
            }
            return Err("校外登录未进入学校统一认证页面".into());
        }
        let execution = execution(&html, "loginForm")?;
        if input_named(&html, "captcha") {
            return Err("统一认证需要图形验证码，请在学校 WebVPN 网页完成验证".into());
        }
        let fields = vec![
            ("username", account.to_string()),
            ("password", password.to_string()),
            (
                "submit",
                input_value(&html, "submit").unwrap_or_else(|| "登录".into()),
            ),
            ("type", "username_password".into()),
            ("execution", execution),
            ("_eventId", "submit".into()),
        ];
        let (url, html) = post(&mut session, url, &fields).await?;
        self.finish_or_challenge(account, session, url, html, None)
            .await
    }
    pub(crate) async fn verify(
        &mut self,
        id: &str,
        token: &str,
        resend: bool,
    ) -> Result<LoginStatus, String> {
        let pending = self
            .pending
            .as_mut()
            .filter(|pending| pending.id == id && Instant::now() < pending.expires)
            .ok_or("验证已过期，请重新登录 WebVPN")?;
        if resend {
            if pending
                .last_send
                .is_some_and(|time| time.elapsed() < Duration::from_secs(60))
            {
                return Err("请稍后再发送验证码".into());
            }
            pending.last_send = Some(Instant::now());
        } else {
            if token.len() != 6 || !token.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err("请输入 6 位短信验证码".into());
            }
            if pending
                .last_submit
                .is_some_and(|time| time.elapsed() < Duration::from_secs(3))
            {
                return Err("请稍后再提交验证码".into());
            }
            pending.last_submit = Some(Instant::now());
        }
        let fields = vec![
            ("execution", pending.execution.clone()),
            (
                "_eventId",
                if resend { "sendToken" } else { "submitToken" }.into(),
            ),
            ("token", if resend { String::new() } else { token.into() }),
        ];
        let (url, html) = post(&mut pending.session, pending.action.clone(), &fields).await?;
        let pending = self.pending.take().ok_or("验证已取消")?;
        self.finish_or_challenge(
            &pending.account,
            pending.session,
            url,
            html,
            Some(ChallengeProgress {
                id: pending.id,
                expires: pending.expires,
                last_send: pending.last_send,
                last_submit: pending.last_submit,
                resend,
            }),
        )
        .await
    }
    fn accept(&mut self, account: &str, mut session: Session) -> LoginStatus {
        self.pending = None;
        session.id = challenge_id();
        self.ready = Some((account.into(), session, Instant::now()));
        LoginStatus {
            stage: "ready",
            challenge_id: None,
            message: "校外连接已就绪".into(),
        }
    }
    async fn finish_or_challenge(
        &mut self,
        account: &str,
        mut session: Session,
        url: Url,
        html: String,
        previous: Option<ChallengeProgress>,
    ) -> Result<LoginStatus, String> {
        if is_cas_login(&url) && html.contains("formToken") {
            let execution = execution(&html, "formToken")?;
            let progress = previous.unwrap_or_else(|| ChallengeProgress {
                id: challenge_id(),
                expires: Instant::now() + Duration::from_secs(5 * 60),
                last_send: None,
                last_submit: None,
                resend: false,
            });
            let message = if progress.last_submit.is_some() && !progress.resend {
                "验证码未通过，请检查后重试"
            } else if progress.resend {
                "验证码已请求，请查看短信"
            } else {
                "请完成学校要求的短信验证"
            };
            self.pending = Some(Pending {
                session,
                account: account.into(),
                action: url,
                execution,
                id: progress.id.clone(),
                expires: progress.expires,
                last_send: progress.last_send,
                last_submit: progress.last_submit,
            });
            return Ok(LoginStatus {
                stage: "sms",
                challenge_id: Some(progress.id),
                message: message.into(),
            });
        }
        if verified_account(&mut session, account).await {
            return Ok(self.accept(account, session));
        }
        Err("WebVPN 登录未通过，请核对统一认证账号和密码".into())
    }
}

fn challenge_id() -> String {
    use aes_gcm::aead::{rand_core::RngCore, OsRng};
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn login_url() -> Url {
    let mut url = Url::parse(&format!("{ORIGIN}{CAS_PREFIX}/login")).unwrap();
    url.query_pairs_mut()
        .append_pair("service", &format!("{ORIGIN}/login?cas_login=true"));
    url
}
fn is_cas_login(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some(HOST)
        && url.port_or_known_default() == Some(443)
        && url.path() == format!("{CAS_PREFIX}/login")
        && url.query_pairs().count() == 1
        && url.query_pairs().any(|(key, value)| {
            key == "service" && value == format!("{ORIGIN}/login?cas_login=true")
        })
}
fn validate(url: &Url) -> Result<(), String> {
    if url.scheme() != "https"
        || url.host_str() != Some(HOST)
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("校外服务跳转地址不受信任".into());
    }
    let path = url.path();
    if is_cas_login(url)
        || matches!(path, "/" | "/login" | "/token-login" | "/user/info")
        || path == format!("{PORTAL_PREFIX}/login")
        || path == format!("{PORTAL_PREFIX}/wengine-vpn-token-login")
    {
        Ok(())
    } else {
        Err("校外认证跳转路径不受支持".into())
    }
}
fn execution(html: &str, id: &str) -> Result<String, String> {
    let (form, contents) = tag_blocks(html, "form")
        .into_iter()
        .find(|(tag, _)| attribute(tag, "id").as_deref() == Some(id))
        .ok_or("统一认证页面格式发生变化")?;
    if attribute(form, "method").is_none_or(|value| !value.eq_ignore_ascii_case("post"))
        || attribute(form, "action").is_some_and(|value| !value.is_empty())
    {
        return Err("统一认证表单地址发生变化".into());
    }
    input_value(contents, "execution")
        .filter(|value| !value.is_empty() && value.len() <= 65536)
        .ok_or_else(|| "统一认证页面缺少有效状态".into())
}
async fn body(response: Response) -> Result<String, String> {
    use futures_util::StreamExt;
    let mut bytes = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| "校外服务响应读取失败")?;
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err("校外服务响应过大".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
async fn get(session: &mut Session, mut url: Url) -> Result<(Url, String), String> {
    for _ in 0..10 {
        validate(&url)?;
        let mut request = session
            .client
            .get(url.clone())
            .header(ACCEPT, "text/html,application/json")
            .header("Cache-Control", "no-cache");
        if let Some(cookie) = session.cookies.header(&url) {
            request = request.header(COOKIE, cookie);
        }
        let response = request
            .send()
            .await
            .map_err(|_| "校外服务连接失败，请稍后重试")?;
        session.cookies.absorb(&url, response.headers());
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or("校外重定向缺少地址")?;
            url = url.join(location).map_err(|_| "校外重定向无效")?;
            continue;
        }
        if !response.status().is_success() {
            return Err(format!("校外服务返回 HTTP {}", response.status().as_u16()));
        }
        return Ok((url, body(response).await?));
    }
    Err("校外认证跳转过多".into())
}
async fn post(
    session: &mut Session,
    url: Url,
    fields: &[(&str, String)],
) -> Result<(Url, String), String> {
    validate(&url)?;
    if !is_cas_login(&url) {
        return Err("统一认证提交地址不受信任".into());
    }
    let mut request = session
        .client
        .post(url.clone())
        .header(reqwest::header::ORIGIN, ORIGIN)
        .header(REFERER, url.as_str())
        .form(fields);
    if let Some(cookie) = session.cookies.header(&url) {
        request = request.header(COOKIE, cookie);
    }
    let response = request
        .send()
        .await
        .map_err(|_| "统一认证响应未确认，请重新打开校外访问")?;
    session.cookies.absorb(&url, response.headers());
    if matches!(response.status().as_u16(), 307 | 308) {
        return Err("统一认证要求重复提交凭据，已停止".into());
    }
    if response.status().is_redirection() {
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or("统一认证重定向缺少地址")?;
        return get(
            session,
            url.join(location).map_err(|_| "统一认证重定向无效")?,
        )
        .await;
    }
    if !response.status().is_success() {
        return Err(format!("统一认证返回 HTTP {}", response.status().as_u16()));
    }
    Ok((url, body(response).await?))
}
async fn verified_account(session: &mut Session, account: &str) -> bool {
    let Ok((url, text)) = get(session, Url::parse(&format!("{ORIGIN}/user/info")).unwrap()).await
    else {
        return false;
    };
    url.path() == "/user/info"
        && serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .is_some_and(|value| value["username"].as_str() == Some(account))
}

pub(super) fn wire_url(url: &Url) -> Url {
    let mut wire = Url::parse(ORIGIN).unwrap();
    wire.set_path(&format!("{BILLING_PREFIX}{}", url.path()));
    wire.set_query(url.query());
    wire
}
pub(super) fn canonical_url(mut url: Url) -> Url {
    if url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && matches!(url.host_str(), Some(HOST | BILLING_HOST))
        && url.username().is_empty()
        && url.password().is_none()
    {
        if let Some(path) = url
            .path()
            .strip_prefix(BILLING_PREFIX)
            .filter(|path| path.starts_with('/'))
        {
            let path = path.to_string();
            let _ = url.set_host(Some(BILLING_HOST));
            url.set_path(&path);
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_school_sso_redirects_are_accepted() {
        assert!(validate(&login_url()).is_ok());
        for raw in [
            "http://webvpn.bjut.edu.cn/login",
            "https://webvpn.bjut.edu.cn.evil.test/login",
            "https://webvpn.bjut.edu.cn:8443/login",
            "https://u:p@webvpn.bjut.edu.cn/login",
            "https://webvpn.bjut.edu.cn/https/untrusted/login",
        ] {
            assert!(validate(&Url::parse(raw).unwrap()).is_err());
        }
        let mut wrong = login_url();
        wrong.set_query(Some("service=https://evil.test/"));
        assert!(validate(&wrong).is_err());
    }
    #[test]
    fn proxy_routes_preserve_billing_paths_and_do_not_accept_other_hosts() {
        let url = Url::parse("https://jfself.bjut.edu.cn/Self/dashboard?q=1").unwrap();
        let wire = wire_url(&url);
        assert_eq!(wire.host_str(), Some(HOST));
        assert!(wire.path().starts_with(BILLING_PREFIX));
        assert_eq!(canonical_url(wire), url);
        let foreign = Url::parse(&format!(
            "https://evil.test{BILLING_PREFIX}/Self/login/verify"
        ))
        .unwrap();
        assert_eq!(canonical_url(foreign.clone()), foreign);
    }
    #[test]
    fn cas_execution_can_exceed_credential_manager_limits() {
        let value = "x".repeat(16000);
        let html=format!("<form id='formToken' method='post' action=''><input name='execution' value='{value}'><input name='token'></form>");
        assert_eq!(execution(&html, "formToken").unwrap(), value);
        assert!(execution(
            &html.replace("action=''", "action='https://evil.test/'"),
            "formToken"
        )
        .is_err());
    }

    #[test]
    fn sms_challenges_expire_and_do_not_survive_credential_changes() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let mut manager = Manager::default();
        manager.bind_credentials([1; 32]);
        let session = Session {
            id: "fixture".into(),
            client: Client::new(),
            cookies: CookieJar::default(),
        };
        let html = "<form id='formToken' method='post' action=''><input name='execution' value='fixture-state'><input name='token'></form>";
        let result = manager
            .finish_or_challenge("fixture", session.clone(), login_url(), html.into(), None)
            .await
            .unwrap();
        assert_eq!(result.stage, "sms");
        let id = result.challenge_id.unwrap();
        // Every verification below must stop locally before any HTTP request.
        assert!(manager
            .verify(&id, "12", false)
            .await
            .err().unwrap()
            .contains("6 位"));
        assert!(manager.verify("different", "123456", false).await.is_err());
        manager.pending.as_mut().unwrap().last_send = Some(Instant::now());
        assert!(manager
            .verify(&id, "", true)
            .await
            .err().unwrap()
            .contains("稍后"));
        manager.pending.as_mut().unwrap().expires = Instant::now() - Duration::from_secs(1);
        assert!(manager
            .verify(&id, "123456", false)
            .await
            .err().unwrap()
            .contains("过期"));
        manager.cancel("different");
        assert!(manager.pending.is_some());
        manager.cancel(&id);
        assert!(manager.pending.is_none());
        manager.accept("fixture", session);
        let first_id = manager.session().unwrap().id;
        manager.bind_credentials([1; 32]);
        assert_eq!(manager.session().unwrap().id, first_id);
        manager.bind_credentials([2; 32]);
        assert!(manager.session().is_err());
        });
    }

    #[test]
    fn shared_upstream_cookie_cannot_substitute_another_billing_account() {
        let html = r#"<script>(function (user) { window.user = user || {}; })({"userName":"fixture-a","leftMoney":1});</script>"#;
        assert!(dashboard_account_matches(html, "fixture-a"));
        assert!(!dashboard_account_matches(html, "fixture-b"));
        assert!(!dashboard_account_matches(
            "<html>学校 WebVPN 登录</html>",
            "fixture-a"
        ));
    }

    #[test]
    fn rewritten_billing_forms_are_validated_without_losing_cookie_scope() {
        let base = Url::parse(BILLING_LOGIN_URL).unwrap();
        let html = format!("<form method='post' action='{BILLING_PREFIX}/Self/login/verify;jsessionid=fixture'><input name='account'><input name='password'><input name='code'><input name='checkcode' value='1234'></form>");
        let action = login_action_for(&html, &base, VpnCompatibility::High).unwrap();
        assert_eq!(action.host_str(), Some(BILLING_HOST));
        assert_eq!(action.path(), "/Self/login/verify;jsessionid=fixture");
        let mut cookies = CookieJar::default();
        let mut headers = HeaderMap::new();
        headers.insert(
            SET_COOKIE,
            "wengine_vpn_ticketwebvpn_bjut_edu_cn=fixture; Path=/; Secure; HttpOnly"
                .parse()
                .unwrap(),
        );
        cookies.absorb(&Url::parse(ORIGIN).unwrap(), &headers);
        assert!(cookies.header(&base).is_none());
        assert!(cookies
            .header(&wire_url(&action))
            .unwrap()
            .contains("=fixture"));
    }
}
