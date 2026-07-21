use reqwest::header::{
    HeaderMap, ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, COOKIE, LOCATION, ORIGIN, REFERER, SET_COOKIE,
};
use reqwest::{Client, Response, Url};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CAS_HOST: &str = "cas.bjut.edu.cn";
const UC_HOST: &str = "uc.bjut.edu.cn";
const ITS_HOST: &str = "itsapp.bjut.edu.cn";
const YD_HOST: &str = "ydapp.bjut.edu.cn";
const UC_ORIGIN: &str = "https://uc.bjut.edu.cn";
const YD_ORIGIN: &str = "https://ydapp.bjut.edu.cn";
const UC_LOGIN_TARGET: &str = "https://uc.bjut.edu.cn/#/user/login";
const CAS_LOGIN_ENTRY: &str = "https://cas.bjut.edu.cn/login?service=https%3A%2F%2Fuc.bjut.edu.cn%2Fapi%2Flogin%3Ftarget%3Dhttps%253A%252F%252Fuc.bjut.edu.cn%252F%2523%252Fuser%252Flogin";
const YD_ENTRY: &str = "https://ydapp.bjut.edu.cn/openV8HomePage";
const YD_APP_ID: &str = "200220816093810809";
const ALIPAY_GATEWAY_HOST: &str = "openapi.alipay.com";
const ALIPAY_APP_ID: &str = "2021003142658367";
const ALIPAY_NOTIFY_URL: &str = "https://alipay.bjut.edu.cn/PayPreService/aliPayWapBackResNotify";
const ALIPAY_RETURN_URL: &str = "https://alipay.bjut.edu.cn/PayPreService/aliPayWapBackResReturn";
const WECHAT_USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 13; M2102K1C Build/TKQ1.220829.002; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/116.0.0.0 Mobile Safari/537.36 XWEB/1160065 MMWEBSDK/20231202 MicroMessenger/8.0.47.2560(0x28002F30) WeChat/arm64 Weixin NetType/WIFI Language/zh_CN ABI/arm64";
const MAX_REDIRECTS: usize = 12;
const CONFIRMATION_LIFETIME: Duration = Duration::from_secs(120);
static CONFIRMATION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub(crate) struct CampusServiceError(String);

impl CampusServiceError {
    fn network(stage: &str, error: reqwest::Error) -> Self {
        Self(format!(
            "校园服务暂不可达（{stage}）：{}",
            classify_network_error(&error)
        ))
    }

    fn protocol(message: impl Into<String>) -> Self {
        Self(format!("校园服务响应格式异常：{}", message.into()))
    }

    fn rejected(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub(crate) fn user_message(self) -> String {
        self.0
    }
}

fn classify_network_error(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        return "请求超时，请检查 VPN 路由或稍后重试";
    }

    let mut chain = String::new();
    let mut source = error.source();
    for _ in 0..6 {
        let Some(current) = source else { break };
        chain.push(' ');
        chain.push_str(&current.to_string().to_ascii_lowercase());
        source = current.source();
    }

    if chain.contains("dns")
        || chain.contains("name resolution")
        || chain.contains("failed to lookup")
        || chain.contains("no such host")
    {
        "DNS 解析失败，请检查 VPN、私人 DNS 或网络设置"
    } else if chain.contains("certificate")
        || chain.contains("unknownissuer")
        || chain.contains("invalid peer")
        || chain.contains("tls")
        || chain.contains("ssl")
    {
        "TLS 握手或证书验证失败，请检查系统时间和 VPN HTTPS 代理"
    } else if chain.contains("network is unreachable") || chain.contains("no route to host") {
        "系统没有到目标服务的可用路由，请检查 VPN 分流规则"
    } else if chain.contains("connection refused") {
        "目标服务拒绝连接"
    } else if chain.contains("connection reset")
        || chain.contains("broken pipe")
        || chain.contains("unexpected eof")
    {
        "连接被中途断开，请检查 VPN 或网络稳定性"
    } else if error.is_connect() {
        "连接失败，请检查 VPN 分流、DNS 和系统网络权限"
    } else if error.is_builder() {
        "请求构建失败"
    } else if error.is_body() {
        "响应传输中断"
    } else if error.is_decode() {
        "响应内容无法解码"
    } else {
        "请求发送失败，请检查 VPN 和网络连接"
    }
}

fn request_stage(url: &Url) -> &'static str {
    match (url.host_str(), url.path()) {
        (Some(CAS_HOST), "/login") => "打开 CAS 登录页",
        (Some(UC_HOST), "/api/login") => "进入统一认证",
        (Some(UC_HOST), "/api/uc/status") => "校验统一认证会话",
        (Some(UC_HOST), "/api/uc/password") => "提交统一认证密码修改",
        (Some(ITS_HOST), "/uc/api/oauth/index") => "通过移动门户 OAuth 中转",
        (Some(YD_HOST), "/openV8HomePage") => "进入移动门户",
        (Some(YD_HOST), "/netpay/openNetPay") => "读取校园卡充值入口",
        (Some(YD_HOST), "/channel/queryNetAccBalance")
        | (Some(YD_HOST), "/channel/get085Detail") => "查询目标网费账户",
        (Some(YD_HOST), "/netpay/createPreThirdTrade") => "创建充值订单",
        (Some(YD_HOST), "/netpay/consumeFromYktToNet") => "提交校园卡扣费",
        (Some(CAS_HOST), _) => "访问 CAS",
        (Some(UC_HOST), _) => "访问统一认证",
        (Some(ITS_HOST), _) => "访问移动门户 OAuth 中转",
        (Some(YD_HOST), _) => "访问移动门户",
        _ => "访问校园服务",
    }
}

#[derive(Clone, Debug)]
struct StoredCookie {
    name: String,
    value: String,
    domain: String,
    host_only: bool,
    path: String,
}

#[derive(Clone, Debug, Default)]
struct DomainCookies {
    values: BTreeMap<(String, String, String), StoredCookie>,
}

impl DomainCookies {
    fn absorb(&mut self, url: &Url, headers: &HeaderMap) {
        let Some(response_host) = url.host_str().map(str::to_ascii_lowercase) else {
            return;
        };
        for raw in headers.get_all(SET_COOKIE).iter() {
            let Ok(raw) = raw.to_str() else { continue };
            let mut parts = raw.split(';');
            let Some(pair) = parts.next() else { continue };
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            let name = name.trim();
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
            {
                continue;
            }
            let mut domain = response_host.clone();
            let mut host_only = true;
            let mut path = default_cookie_path(url.path());
            let mut expired = value.trim().is_empty();
            for attribute in parts {
                let (key, value) = attribute
                    .trim()
                    .split_once('=')
                    .map(|(key, value)| (key.trim(), value.trim()))
                    .unwrap_or((attribute.trim(), ""));
                match key.to_ascii_lowercase().as_str() {
                    "domain" => {
                        let candidate = value.trim_start_matches('.').to_ascii_lowercase();
                        if response_host == candidate
                            || response_host.ends_with(&format!(".{candidate}"))
                        {
                            domain = candidate;
                            host_only = false;
                        } else {
                            domain.clear();
                        }
                    }
                    "path" if value.starts_with('/') => path = value.to_string(),
                    "max-age" if value.parse::<i64>().ok().is_some_and(|age| age <= 0) => {
                        expired = true
                    }
                    _ => {}
                }
            }
            if domain.is_empty() {
                continue;
            }
            let key = (domain.clone(), path.clone(), name.to_string());
            if expired {
                self.values.remove(&key);
            } else {
                self.values.insert(
                    key,
                    StoredCookie {
                        name: name.to_string(),
                        value: value.trim().to_string(),
                        domain,
                        host_only,
                        path,
                    },
                );
            }
        }
    }

    fn header(&self, url: &Url) -> Option<String> {
        let host = url.host_str()?.to_ascii_lowercase();
        let path = url.path();
        let mut cookies = self
            .values
            .values()
            .filter(|cookie| {
                let domain_matches = if cookie.host_only {
                    host == cookie.domain
                } else {
                    host == cookie.domain || host.ends_with(&format!(".{}", cookie.domain))
                };
                domain_matches && cookie_path_matches(path, &cookie.path)
            })
            .collect::<Vec<_>>();
        cookies.sort_by_key(|cookie| std::cmp::Reverse(cookie.path.len()));
        (!cookies.is_empty()).then(|| {
            cookies
                .into_iter()
                .map(|cookie| format!("{}={}", cookie.name, cookie.value))
                .collect::<Vec<_>>()
                .join("; ")
        })
    }
}

fn default_cookie_path(path: &str) -> String {
    if !path.starts_with('/') || path == "/" {
        return "/".to_string();
    }
    path.rsplit_once('/')
        .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
        .unwrap_or("/")
        .to_string()
}

fn cookie_path_matches(request: &str, cookie: &str) -> bool {
    request == cookie
        || request.starts_with(cookie)
            && (cookie.ends_with('/') || request.as_bytes().get(cookie.len()) == Some(&b'/'))
}

#[derive(Clone, Debug)]
struct CampusSession {
    client: Client,
    cookies: DomainCookies,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RechargeApi {
    target_query_path: &'static str,
    factory_code: &'static str,
}

impl RechargeApi {
    fn from_route(route: &str) -> Result<Self, CampusServiceError> {
        match route {
            // The current portal still advertises the legacy-named SPA route,
            // while both captured recharge paths use get085Detail with N006.
            "/pages/recharge/networkFeeCharge/networkFeeCharge"
            | "/pages/recharge/networkFeeCharge/networkFeeChargeNew" => Ok(Self {
                target_query_path: "/channel/get085Detail",
                factory_code: "N006",
            }),
            _ => Err(CampusServiceError::protocol("充值入口返回了未知的业务通道")),
        }
    }
}

#[derive(Clone, Debug)]
struct RechargeContext {
    payer_account: String,
    card_balance: String,
    allowed_time: String,
    openid: String,
    api: RechargeApi,
}

#[derive(Debug)]
pub(crate) struct PendingRecharge {
    confirmation_id: String,
    created_at: Instant,
    session: CampusSession,
    payer_account: String,
    target_account: String,
    amount: String,
    target_balance: String,
    target_status_code: String,
    openid: String,
    api: RechargeApi,
}

impl PendingRecharge {
    pub(crate) fn confirmation_id(&self) -> &str {
        &self.confirmation_id
    }

    pub(crate) fn expired(&self) -> bool {
        self.created_at.elapsed() > CONFIRMATION_LIFETIME
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RechargePreview {
    pub confirmation_id: String,
    pub payer_account: String,
    pub card_balance: String,
    pub target_account: String,
    pub target_balance: String,
    pub target_status: String,
    pub amount: String,
    pub allowed_time: String,
    pub expires_in_seconds: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RechargeResult {
    pub message: String,
    pub target_account: String,
    pub amount: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RechargeBalanceSnapshot {
    pub payer_account: String,
    pub card_balance: String,
    pub target_account: String,
    pub target_balance: String,
    pub target_status: String,
}

#[derive(Debug)]
pub(crate) struct PendingAlipayRecharge {
    confirmation_id: String,
    created_at: Instant,
    session: CampusSession,
    payer_account: String,
    amount: String,
    openid: String,
}

impl PendingAlipayRecharge {
    pub(crate) fn confirmation_id(&self) -> &str {
        &self.confirmation_id
    }

    pub(crate) fn expired(&self) -> bool {
        self.created_at.elapsed() > CONFIRMATION_LIFETIME
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AlipayRechargePreview {
    pub confirmation_id: String,
    pub payer_account: String,
    pub card_balance: String,
    pub amount: String,
    pub expires_in_seconds: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AlipayRechargeResult {
    pub message: String,
    pub payer_account: String,
    pub amount: String,
    pub payment_url: String,
}

pub(crate) async fn change_password(
    account: &str,
    saved_password: &str,
    supplied_password: &str,
    new_password: &str,
) -> Result<String, CampusServiceError> {
    if supplied_password != saved_password {
        return Err(CampusServiceError::rejected(
            "当前密码与 App 中已保存的统一认证密码不一致",
        ));
    }
    validate_new_password(new_password, supplied_password)?;
    let mut session = authenticate(account, saved_password).await?;
    let response = post_json(
        &mut session,
        parse_url("https://uc.bjut.edu.cn/api/uc/password")?,
        json!({"oldPassword": supplied_password, "newPassword": new_password}),
        UC_ORIGIN,
        "https://uc.bjut.edu.cn/#/user/login",
        false,
    )
    .await?;
    ensure_code_zero(&response, "统一认证密码修改")?;
    Ok("统一认证密码修改成功".to_string())
}

pub(crate) async fn prepare_recharge(
    account: &str,
    password: &str,
    target_account: &str,
    amount: &str,
) -> Result<(PendingRecharge, RechargePreview), CampusServiceError> {
    let target_account = validate_target_account(target_account)?;
    let amount = canonical_amount(amount)?;
    let mut session = authenticate(account, password).await?;
    let context = enter_recharge(&mut session).await?;
    let (target_balance, target_status_code, target_status) =
        query_target_account(&mut session, &context, &target_account).await?;
    if !matches!(target_status_code.as_str(), "" | "0" | "1" | "2") {
        return Err(CampusServiceError::rejected("目标网费账户状态不允许充值"));
    }
    if let (Some(available), Some(required)) = (
        parse_money_cents(&context.card_balance),
        parse_money_cents(&amount),
    ) {
        if required > available {
            return Err(CampusServiceError::rejected(format!(
                "校园卡余额不足：当前 {} 元，本次需要 {} 元",
                context.card_balance, amount
            )));
        }
    }

    let confirmation_id = confirmation_id();
    let preview = RechargePreview {
        confirmation_id: confirmation_id.clone(),
        payer_account: context.payer_account.clone(),
        card_balance: context.card_balance.clone(),
        target_account: target_account.clone(),
        target_balance: target_balance.clone(),
        target_status: target_status.clone(),
        amount: amount.clone(),
        allowed_time: context.allowed_time.clone(),
        expires_in_seconds: CONFIRMATION_LIFETIME.as_secs(),
    };
    let pending = PendingRecharge {
        confirmation_id,
        created_at: Instant::now(),
        session,
        payer_account: context.payer_account,
        target_account,
        amount,
        target_balance,
        target_status_code,
        openid: context.openid,
        api: context.api,
    };
    Ok((pending, preview))
}

pub(crate) async fn query_recharge_balances(
    account: &str,
    password: &str,
    target_account: &str,
) -> Result<RechargeBalanceSnapshot, CampusServiceError> {
    let target_account = validate_target_account(target_account)?;
    let mut session = authenticate(account, password).await?;
    let context = enter_recharge(&mut session).await?;
    let (target_balance, _, target_status) =
        query_target_account(&mut session, &context, &target_account).await?;
    Ok(RechargeBalanceSnapshot {
        payer_account: context.payer_account,
        card_balance: context.card_balance,
        target_account,
        target_balance,
        target_status,
    })
}

pub(crate) async fn execute_recharge(
    mut pending: PendingRecharge,
) -> Result<RechargeResult, CampusServiceError> {
    if pending.expired() {
        return Err(CampusServiceError::rejected(
            "充值确认已过期，请重新核对账户与金额",
        ));
    }
    let factory = pending.api.factory_code;
    let create = yd_post(
        &mut pending.session,
        "/netpay/createPreThirdTrade",
        json!({
            "payamt": pending.amount,
            "idserial": pending.payer_account,
            "netaccno": pending.target_account,
            "factorycode": factory,
            "openid": pending.openid,
            "orgid": "2",
            "netbalance": format_balance_for_trade(&pending.target_balance),
            "netaccstatus": pending.target_status_code,
            "paytype": 2
        }),
    )
    .await
    .map_err(|error| {
        CampusServiceError::rejected(format!(
            "充值订单创建结果未能确认：{}。请先到计费系统核对记录，不要立即重复充值",
            error.user_message()
        ))
    })?;
    ensure_success(&create, "创建充值订单")?;
    let order_numbers = (|| {
        let result = create
            .get("resultData")
            .and_then(Value::as_object)
            .ok_or_else(|| CampusServiceError::protocol("充值订单缺少 resultData"))?;
        Ok::<_, CampusServiceError>((
            required_json_string(result.get("payorderno"), "payorderno")?,
            required_json_string(result.get("partnerjourno"), "partnerjourno")?,
        ))
    })()
    .map_err(|error| {
        CampusServiceError::rejected(format!(
            "充值订单已被服务器接受，但订单编号不完整：{}。请先到计费系统核对记录，不要立即重复充值",
            error.user_message()
        ))
    })?;
    let (pay_order_no, partner_jour_no) = order_numbers;

    let consume = yd_post(
        &mut pending.session,
        "/netpay/consumeFromYktToNet",
        json!({
            "paytxamt": pending.amount,
            "payWay": 2,
            "openid": pending.openid,
            "orgid": "2",
            "idserial": pending.payer_account,
            "netaccno": pending.target_account,
            "payorderno": pay_order_no,
            "partnerjourno": partner_jour_no,
            "factorycode": factory
        }),
    )
    .await
    .map_err(|error| {
        CampusServiceError::rejected(format!(
            "充值订单已经创建，但扣费结果未能确认：{}。请先到计费系统核对记录，不要立即重复充值",
            error.user_message()
        ))
    })?;
    ensure_success(&consume, "校园卡扣费")?;
    Ok(RechargeResult {
        message: format!(
            "已从校园卡为 {} 充值 {} 元",
            pending.target_account, pending.amount
        ),
        target_account: pending.target_account,
        amount: pending.amount,
    })
}

pub(crate) async fn prepare_alipay_card_recharge(
    account: &str,
    password: &str,
    amount: &str,
) -> Result<(PendingAlipayRecharge, AlipayRechargePreview), CampusServiceError> {
    let amount = canonical_amount(amount)?;
    let mut session = authenticate(account, password).await?;
    let context = enter_recharge(&mut session).await?;
    let opened = yd_post(
        &mut session,
        "/cardpay/openCardPay",
        json!({"openid": context.openid}),
    )
    .await?;
    ensure_success(&opened, "打开校园卡支付宝充值")?;

    let confirmation_id = confirmation_id();
    let preview = AlipayRechargePreview {
        confirmation_id: confirmation_id.clone(),
        payer_account: context.payer_account.clone(),
        card_balance: context.card_balance.clone(),
        amount: amount.clone(),
        expires_in_seconds: CONFIRMATION_LIFETIME.as_secs(),
    };
    let pending = PendingAlipayRecharge {
        confirmation_id,
        created_at: Instant::now(),
        session,
        payer_account: context.payer_account,
        amount,
        openid: context.openid,
    };
    Ok((pending, preview))
}

pub(crate) async fn execute_alipay_card_recharge(
    mut pending: PendingAlipayRecharge,
) -> Result<AlipayRechargeResult, CampusServiceError> {
    if pending.expired() {
        return Err(CampusServiceError::rejected(
            "支付宝充值确认已过期，请重新核对校园卡与金额",
        ));
    }
    let html = yd_post_text(
        &mut pending.session,
        "/alipay/transferFromAlipay2Card",
        json!({
            "idserial": pending.payer_account,
            "txamt": pending.amount,
            "payway": 4,
            "openid": pending.openid,
            "payWay": 4,
            "tradetype": "WAP",
            "usertype": "1",
            "orgid": "2"
        }),
    )
    .await
    .map_err(|error| {
        CampusServiceError::rejected(format!(
            "支付宝订单创建结果未能确认：{}。请先检查校园卡充值记录，不要立即重复操作",
            error.user_message()
        ))
    })?;
    let payment_url = alipay_gateway_url(&html, &pending.amount).map_err(|error| {
        CampusServiceError::rejected(format!(
            "支付宝订单已被校园支付平台接受，但支付入口无法安全打开：{}。请先检查校园卡充值记录，不要立即重复操作",
            error.user_message()
        ))
    })?;
    Ok(AlipayRechargeResult {
        message: format!(
            "已为校园卡 {} 创建 {} 元支付宝充值订单",
            pending.payer_account, pending.amount
        ),
        payer_account: pending.payer_account,
        amount: pending.amount,
        payment_url,
    })
}

async fn authenticate(account: &str, password: &str) -> Result<CampusSession, CampusServiceError> {
    if account.trim().is_empty() || password.is_empty() {
        return Err(CampusServiceError::rejected(
            "首选账号缺少统一认证账号或密码",
        ));
    }
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9".parse().unwrap());
    let client = build_client(headers)?;
    let mut session = CampusSession {
        client,
        cookies: DomainCookies::default(),
    };
    let (login_url, login_html) =
        get_follow_text(&mut session, parse_url(CAS_LOGIN_ENTRY)?, &[CAS_HOST], None).await?;
    if login_url.host_str() != Some(CAS_HOST) || login_url.path() != "/login" {
        return Err(CampusServiceError::protocol(
            "统一认证入口没有到达 CAS 登录页",
        ));
    }
    let (action, fields) = cas_login_form(&login_html, &login_url, account, password)?;
    let response = send_form(
        &mut session,
        action.clone(),
        &fields,
        CAS_HOST,
        login_url.as_str(),
    )
    .await?;
    if matches!(response.status().as_u16(), 307 | 308) {
        return Err(CampusServiceError::protocol(
            "CAS 要求重复提交密码，已安全中止",
        ));
    }
    if !response.status().is_redirection() {
        return Err(CampusServiceError::rejected(
            "统一认证拒绝登录；为避免触发验证码，本次没有自动重试",
        ));
    }
    let next = redirect_target(&action, &response, &[CAS_HOST, UC_HOST])?;
    let _ = response.bytes().await;
    let (final_url, _) =
        get_follow_text(&mut session, next, &[CAS_HOST, UC_HOST], Some(&action)).await?;
    if final_url.host_str() != Some(UC_HOST) {
        return Err(CampusServiceError::protocol("登录后未进入 UC"));
    }
    let status = get_json(
        &mut session,
        parse_url("https://uc.bjut.edu.cn/api/uc/status")?,
        UC_ORIGIN,
        "https://uc.bjut.edu.cn/#/user/login",
    )
    .await?;
    ensure_code_zero(&status, "统一认证会话校验")?;
    Ok(session)
}

fn build_client(headers: HeaderMap) -> Result<Client, CampusServiceError> {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .use_native_tls()
        .min_tls_version(reqwest::tls::Version::TLS_1_2)
        .max_tls_version(reqwest::tls::Version::TLS_1_2)
        .user_agent(WECHAT_USER_AGENT)
        .default_headers(headers)
        .build()
        .map_err(|error| CampusServiceError::network("初始化 HTTPS 客户端", error))
}

async fn enter_recharge(
    session: &mut CampusSession,
) -> Result<RechargeContext, CampusServiceError> {
    let (final_url, _) = get_follow_text(
        session,
        parse_url(YD_ENTRY)?,
        &[YD_HOST, CAS_HOST, ITS_HOST, UC_HOST],
        Some(&parse_url("https://uc.bjut.edu.cn/#/user/login")?),
    )
    .await?;
    if final_url.host_str() != Some(YD_HOST) {
        return Err(CampusServiceError::protocol("移动门户没有进入网费充值服务"));
    }
    let openid = extract_openid(&final_url)?;
    let opened = yd_post(session, "/netpay/openNetPay", json!({"openid": openid})).await?;
    ensure_success(&opened, "打开网费充值")?;
    let route = opened
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| CampusServiceError::protocol("充值初始化缺少业务通道"))?;
    let api = RechargeApi::from_route(route)?;
    let data = opened
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| CampusServiceError::protocol("充值初始化缺少 data"))?;
    let card = data
        .get("cardPay")
        .and_then(Value::as_object)
        .ok_or_else(|| CampusServiceError::protocol("充值初始化缺少校园卡信息"))?;
    Ok(RechargeContext {
        payer_account: required_json_string(card.get("idserial"), "idserial")?,
        card_balance: required_json_string(card.get("cardbal"), "cardbal")?,
        allowed_time: data
            .get("allowNettime")
            .and_then(Value::as_str)
            .unwrap_or("以服务器开放时间为准")
            .to_string(),
        openid,
        api,
    })
}

async fn query_target_account(
    session: &mut CampusSession,
    context: &RechargeContext,
    target: &str,
) -> Result<(String, String, String), CampusServiceError> {
    let response = yd_post(
        session,
        context.api.target_query_path,
        json!({
            "idserial": target,
            "netaccno": target,
            "factorycode": context.api.factory_code
        }),
    )
    .await?;
    ensure_success(&response, "查询目标网费账户")?;
    let balance = trim_currency(&required_json_string(
        response.pointer("/resultData/balance"),
        "balance",
    )?);
    let user_type = response
        .pointer("/resultData/user_type")
        .and_then(Value::as_str)
        .unwrap_or("普通账号");
    if user_type == "经费账号" {
        return Err(CampusServiceError::rejected("经费账号不能通过校园卡充值"));
    }
    Ok((balance, String::new(), "可充值".to_string()))
}

async fn yd_post(
    session: &mut CampusSession,
    path: &str,
    body: Value,
) -> Result<Value, CampusServiceError> {
    let url = parse_url(&format!("{YD_ORIGIN}{path}"))?;
    post_json(
        session,
        url,
        body,
        YD_ORIGIN,
        "https://ydapp.bjut.edu.cn/",
        true,
    )
    .await
}

async fn yd_post_text(
    session: &mut CampusSession,
    path: &str,
    body: Value,
) -> Result<String, CampusServiceError> {
    let url = parse_url(&format!("{YD_ORIGIN}{path}"))?;
    validate_url(&url, &[YD_HOST])?;
    let mut request = session
        .client
        .post(url.clone())
        .header(
            ACCEPT,
            "text/html,application/xhtml+xml,application/json;q=0.9,*/*;q=0.8",
        )
        .header(CONTENT_TYPE, "application/json")
        .header(ORIGIN, YD_ORIGIN)
        .header(REFERER, "https://ydapp.bjut.edu.cn/")
        .header("X-Requested-With", "XMLHttpRequest")
        .header("Sec-Fetch-Dest", "empty")
        .header("Sec-Fetch-Mode", "cors")
        .header("Sec-Fetch-Site", "same-origin")
        .header("session-type", "uniapp")
        .header("isWechatApp", "true")
        .header("orgid", "2")
        .json(&body);
    if let Some(cookie) = session.cookies.header(&url) {
        request = request.header(COOKIE, cookie);
    }
    let stage = request_stage(&url);
    let response = request
        .send()
        .await
        .map_err(|error| CampusServiceError::network(stage, error))?;
    session.cookies.absorb(&url, response.headers());
    if !response.status().is_success() {
        return Err(CampusServiceError::protocol(format!(
            "接口返回 HTTP {}",
            response.status().as_u16()
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length > 128 * 1024)
    {
        return Err(CampusServiceError::protocol("支付入口响应体过大"));
    }
    let text = response
        .text()
        .await
        .map_err(|error| CampusServiceError::network(stage, error))?;
    if text.len() > 128 * 1024 {
        return Err(CampusServiceError::protocol("支付入口响应体过大"));
    }
    Ok(text)
}

async fn get_follow_text(
    session: &mut CampusSession,
    mut url: Url,
    allowed_hosts: &[&str],
    referer: Option<&Url>,
) -> Result<(Url, String), CampusServiceError> {
    let mut current_referer = referer.map(Url::to_string);
    for _ in 0..=MAX_REDIRECTS {
        validate_url(&url, allowed_hosts)?;
        let mut request = session
            .client
            .get(url.clone())
            .header(
                ACCEPT,
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
            )
            .header("Sec-Fetch-Dest", "document")
            .header("Sec-Fetch-Mode", "navigate")
            .header("Upgrade-Insecure-Requests", "1");
        if let Some(cookie) = session.cookies.header(&url) {
            request = request.header(COOKIE, cookie);
        }
        if let Some(value) = current_referer.as_deref() {
            request = request.header(REFERER, value);
        }
        let stage = request_stage(&url);
        let response = request
            .send()
            .await
            .map_err(|error| CampusServiceError::network(stage, error))?;
        session.cookies.absorb(&url, response.headers());
        if response.status().is_redirection() {
            let next = redirect_target(&url, &response, allowed_hosts)?;
            let _ = response.bytes().await;
            current_referer = Some(url.to_string());
            url = next;
            continue;
        }
        if !response.status().is_success() {
            return Err(CampusServiceError::protocol(format!(
                "{} 返回 HTTP {}",
                url.host_str().unwrap_or("服务器"),
                response.status().as_u16()
            )));
        }
        let text = response
            .text()
            .await
            .map_err(|error| CampusServiceError::network(stage, error))?;
        if url.host_str() == Some(ITS_HOST)
            && matches!(url.path(), "/uc/api/oauth/index" | "/a_bjut/api/sso/index")
        {
            let next = solve_its_challenge(&url, &text)?;
            current_referer = Some(url.to_string());
            url = next;
            continue;
        }
        return Ok((url, text));
    }
    Err(CampusServiceError::protocol("校园服务重定向次数过多"))
}

async fn get_json(
    session: &mut CampusSession,
    url: Url,
    origin: &str,
    referer: &str,
) -> Result<Value, CampusServiceError> {
    validate_url(&url, &[url.host_str().unwrap_or_default()])?;
    let mut request = session
        .client
        .get(url.clone())
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(ORIGIN, origin)
        .header(REFERER, referer)
        .header("X-Requested-With", "XMLHttpRequest")
        .header("Sec-Fetch-Dest", "empty")
        .header("Sec-Fetch-Mode", "cors")
        .header("Sec-Fetch-Site", "same-origin");
    if let Some(cookie) = session.cookies.header(&url) {
        request = request.header(COOKIE, cookie);
    }
    let stage = request_stage(&url);
    let response = request
        .send()
        .await
        .map_err(|error| CampusServiceError::network(stage, error))?;
    session.cookies.absorb(&url, response.headers());
    response_json(response, stage).await
}

async fn post_json(
    session: &mut CampusSession,
    url: Url,
    body: Value,
    origin: &str,
    referer: &str,
    yd_headers: bool,
) -> Result<Value, CampusServiceError> {
    validate_url(&url, &[url.host_str().unwrap_or_default()])?;
    let mut request = session
        .client
        .post(url.clone())
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(CONTENT_TYPE, "application/json")
        .header(ORIGIN, origin)
        .header(REFERER, referer)
        .header("X-Requested-With", "XMLHttpRequest")
        .header("Sec-Fetch-Dest", "empty")
        .header("Sec-Fetch-Mode", "cors")
        .header("Sec-Fetch-Site", "same-origin")
        .json(&body);
    if yd_headers {
        request = request
            .header("session-type", "uniapp")
            .header("isWechatApp", "true")
            .header("orgid", "2");
    }
    if let Some(cookie) = session.cookies.header(&url) {
        request = request.header(COOKIE, cookie);
    }
    let stage = request_stage(&url);
    let response = request
        .send()
        .await
        .map_err(|error| CampusServiceError::network(stage, error))?;
    session.cookies.absorb(&url, response.headers());
    response_json(response, stage).await
}

async fn response_json(response: Response, stage: &str) -> Result<Value, CampusServiceError> {
    let status = response.status();
    if !status.is_success() {
        return Err(CampusServiceError::protocol(format!(
            "接口返回 HTTP {}",
            status.as_u16()
        )));
    }
    match response.json::<Value>().await {
        Ok(value) => Ok(value),
        Err(error) if error.is_decode() => {
            Err(CampusServiceError::protocol("接口没有返回有效 JSON"))
        }
        Err(error) => Err(CampusServiceError::network(stage, error)),
    }
}

async fn send_form(
    session: &mut CampusSession,
    url: Url,
    fields: &[(String, String)],
    origin_host: &str,
    referer: &str,
) -> Result<Response, CampusServiceError> {
    validate_url(&url, &[origin_host])?;
    let mut request = session
        .client
        .post(url.clone())
        .header(
            ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
        )
        .header(ORIGIN, format!("https://{origin_host}"))
        .header(REFERER, referer)
        .header("Sec-Fetch-Dest", "document")
        .header("Sec-Fetch-Mode", "navigate")
        .header("Sec-Fetch-Site", "same-origin")
        .header("Sec-Fetch-User", "?1")
        .header("Upgrade-Insecure-Requests", "1")
        .form(fields);
    if let Some(cookie) = session.cookies.header(&url) {
        request = request.header(COOKIE, cookie);
    }
    let response = request
        .send()
        .await
        .map_err(|error| CampusServiceError::network("提交 CAS 登录", error))?;
    session.cookies.absorb(&url, response.headers());
    Ok(response)
}

fn alipay_gateway_url(html: &str, expected_amount: &str) -> Result<String, CampusServiceError> {
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(relative) = lower[cursor..].find("<form") {
        let start = cursor + relative;
        let Some(open_end_relative) = html[start..].find('>') else {
            break;
        };
        let open_end = start + open_end_relative + 1;
        let Some(close_relative) = lower[open_end..].find("</form>") else {
            break;
        };
        let close = open_end + close_relative;
        let opening = &html[start..open_end];
        let action_value = decode_html_entities(&attribute(opening, "action").unwrap_or_default());
        let Ok(mut action) = Url::parse(&action_value) else {
            cursor = close + 7;
            continue;
        };
        if action.host_str() != Some(ALIPAY_GATEWAY_HOST) || action.path() != "/gateway.do" {
            cursor = close + 7;
            continue;
        }
        validate_url(&action, &[ALIPAY_GATEWAY_HOST])?;
        if action.fragment().is_some()
            || !attribute(opening, "method")
                .unwrap_or_else(|| "get".to_string())
                .eq_ignore_ascii_case("post")
        {
            return Err(CampusServiceError::protocol(
                "支付宝支付表单不是受支持的 HTTPS POST",
            ));
        }

        let values = unique_query_values(&action)?;
        let expected_keys = [
            "alipay_sdk",
            "app_id",
            "charset",
            "format",
            "method",
            "notify_url",
            "return_url",
            "sign",
            "sign_type",
            "timestamp",
            "version",
        ];
        if values.len() != expected_keys.len()
            || !expected_keys.iter().all(|key| values.contains_key(*key))
            || values.get("app_id").map(String::as_str) != Some(ALIPAY_APP_ID)
            || values.get("method").map(String::as_str) != Some("alipay.trade.wap.pay")
            || values.get("format").map(String::as_str) != Some("json")
            || values.get("charset").map(String::as_str) != Some("UTF-8")
            || values.get("sign_type").map(String::as_str) != Some("RSA2")
            || values.get("version").map(String::as_str) != Some("1.0")
            || values.get("notify_url").map(String::as_str) != Some(ALIPAY_NOTIFY_URL)
            || values.get("return_url").map(String::as_str) != Some(ALIPAY_RETURN_URL)
            || values
                .get("sign")
                .is_none_or(|value| !(32..=1024).contains(&value.len()))
            || values.get("timestamp").is_none_or(String::is_empty)
        {
            return Err(CampusServiceError::protocol(
                "支付宝网关签名参数与已验证流程不一致",
            ));
        }

        let body = &html[open_end..close];
        let mut form_fields = BTreeMap::<String, String>::new();
        for tag in tags(body, "input") {
            let Some(name) = attribute(tag, "name") else {
                continue;
            };
            let input_type = attribute(tag, "type")
                .unwrap_or_else(|| "text".to_string())
                .to_ascii_lowercase();
            if matches!(input_type.as_str(), "button" | "file" | "reset" | "submit") {
                continue;
            }
            let value = decode_html_entities(&attribute(tag, "value").unwrap_or_default());
            if form_fields.insert(name, value).is_some() {
                return Err(CampusServiceError::protocol("支付宝支付表单字段重复"));
            }
        }
        if form_fields.len() != 1 || !form_fields.contains_key("biz_content") {
            return Err(CampusServiceError::protocol("支付宝支付表单字段发生变化"));
        }
        let mut biz_content = form_fields.remove("biz_content").unwrap_or_default();
        if biz_content.starts_with("%7B") || biz_content.starts_with("%7b") {
            let decoder = Url::parse(&format!("https://placeholder.invalid/?value={biz_content}"))
                .map_err(|_| CampusServiceError::protocol("支付宝订单参数编码无效"))?;
            biz_content = decoder
                .query_pairs()
                .find(|(key, _)| key == "value")
                .map(|(_, value)| value.into_owned())
                .ok_or_else(|| CampusServiceError::protocol("支付宝订单参数编码无效"))?;
        }
        let order: Value = serde_json::from_str(&biz_content)
            .map_err(|_| CampusServiceError::protocol("支付宝订单参数不是有效 JSON"))?;
        let order = order
            .as_object()
            .ok_or_else(|| CampusServiceError::protocol("支付宝订单参数格式无效"))?;
        if order.len() != 4
            || required_json_string(order.get("total_amount"), "total_amount")? != expected_amount
            || order.get("subject").and_then(Value::as_str) != Some("支付宝充值校园卡")
            || order.get("product_code").and_then(Value::as_str) != Some("QUICK_WAP_WAY")
            || required_json_string(order.get("out_trade_no"), "out_trade_no")?
                .bytes()
                .any(|byte| !byte.is_ascii_digit())
        {
            return Err(CampusServiceError::protocol(
                "支付宝订单内容与本次确认不一致",
            ));
        }
        action
            .query_pairs_mut()
            .append_pair("biz_content", &biz_content);
        let payment_url: String = action.into();
        if payment_url.len() > 8192 {
            return Err(CampusServiceError::protocol("支付宝支付地址过长"));
        }
        return Ok(payment_url);
    }
    Err(CampusServiceError::protocol(
        "没有找到受信任的支付宝支付表单",
    ))
}

fn cas_login_form(
    html: &str,
    page_url: &Url,
    account: &str,
    password: &str,
) -> Result<(Url, Vec<(String, String)>), CampusServiceError> {
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(relative) = lower[cursor..].find("<form") {
        let start = cursor + relative;
        let Some(open_end_relative) = html[start..].find('>') else {
            break;
        };
        let open_end = start + open_end_relative + 1;
        let Some(close_relative) = lower[open_end..].find("</form>") else {
            break;
        };
        let close = open_end + close_relative;
        let body = &html[open_end..close];
        if !body.to_ascii_lowercase().contains("name=\"execution\"")
            && !body.to_ascii_lowercase().contains("name='execution'")
        {
            cursor = close + 7;
            continue;
        }
        if body.to_ascii_lowercase().contains("name=\"captcha\"")
            || body.to_ascii_lowercase().contains("name='captcha'")
        {
            return Err(CampusServiceError::rejected(
                "统一认证当前要求图形验证码；本次没有提交账号密码",
            ));
        }
        let opening = &html[start..open_end];
        let action_value = attribute(opening, "action").unwrap_or_default();
        let action = page_url
            .join(&decode_html_entities(&action_value))
            .map_err(|_| CampusServiceError::protocol("CAS 表单地址无效"))?;
        validate_cas_action(&action)?;
        let mut fields = BTreeMap::<String, String>::new();
        for tag in tags(body, "input") {
            let Some(name) = attribute(tag, "name") else {
                continue;
            };
            let input_type = attribute(tag, "type")
                .unwrap_or_else(|| "text".to_string())
                .to_ascii_lowercase();
            if matches!(input_type.as_str(), "button" | "file" | "reset" | "submit") {
                continue;
            }
            if matches!(input_type.as_str(), "checkbox" | "radio")
                && !tag.to_ascii_lowercase().contains("checked")
            {
                continue;
            }
            fields.entry(name).or_insert_with(|| {
                decode_html_entities(&attribute(tag, "value").unwrap_or_default())
            });
        }
        if fields.get("execution").is_none_or(String::is_empty) {
            return Err(CampusServiceError::protocol("CAS 登录页缺少 execution"));
        }
        fields.insert("username".to_string(), account.to_string());
        fields.insert("password".to_string(), password.to_string());
        fields.insert("type".to_string(), "username_password".to_string());
        fields.insert("_eventId".to_string(), "submit".to_string());
        return Ok((action, fields.into_iter().collect()));
    }
    Err(CampusServiceError::protocol("没有找到 CAS 登录表单"))
}

fn validate_cas_action(url: &Url) -> Result<(), CampusServiceError> {
    validate_url(url, &[CAS_HOST])?;
    if url.path() != "/login" {
        return Err(CampusServiceError::protocol("CAS 表单路径发生变化"));
    }
    let mut service = None;
    for (key, value) in url.query_pairs() {
        if key == "service" && service.replace(value.into_owned()).is_some() {
            return Err(CampusServiceError::protocol("CAS service 重复"));
        } else if key != "service" {
            return Err(CampusServiceError::protocol("CAS 表单出现未知参数"));
        }
    }
    let service = parse_url(
        service
            .as_deref()
            .ok_or_else(|| CampusServiceError::protocol("CAS 表单缺少 UC service"))?,
    )?;
    validate_url(&service, &[UC_HOST])?;
    if service.path() != "/api/login"
        || service.query_pairs().collect::<Vec<_>>()
            != vec![("target".into(), UC_LOGIN_TARGET.into())]
    {
        return Err(CampusServiceError::protocol(
            "CAS service 不是预期的 UC 入口",
        ));
    }
    Ok(())
}

fn solve_its_challenge(current: &Url, html: &str) -> Result<Url, CampusServiceError> {
    let marker = "window.location.href";
    let assignment = html
        .find(marker)
        .ok_or_else(|| CampusServiceError::protocol("itsapp 导航挑战格式未知"))?
        + marker.len();
    let assignment_tail = &html[assignment..];
    let equals = assignment_tail
        .find('=')
        .filter(|offset| assignment_tail[..*offset].trim().is_empty())
        .ok_or_else(|| CampusServiceError::protocol("itsapp 导航挑战缺少赋值"))?;
    let value_tail = &assignment_tail[equals + 1..];
    let leading_whitespace = value_tail.len() - value_tail.trim_start().len();
    let start = assignment + equals + 1 + leading_whitespace;
    let quote = *html
        .as_bytes()
        .get(start)
        .filter(|byte| matches!(byte, b'\'' | b'\"'))
        .ok_or_else(|| CampusServiceError::protocol("itsapp 导航地址缺少引号"))?;
    let literal_start = start + 1;
    let literal_end = html[literal_start..]
        .find(quote as char)
        .map(|offset| literal_start + offset)
        .ok_or_else(|| CampusServiceError::protocol("itsapp 导航地址不完整"))?;
    let literal = decode_html_entities(&html[literal_start..literal_end]);
    if !literal.starts_with('?') || literal.contains('\\') {
        return Err(CampusServiceError::protocol("itsapp 导航地址不受支持"));
    }
    let tail = &html[literal_end + 1..];
    let md5_start = tail
        .find("md5(")
        .ok_or_else(|| CampusServiceError::protocol("itsapp 导航挑战缺少 MD5"))?
        + 4;
    let md5_quote = *tail
        .as_bytes()
        .get(md5_start)
        .filter(|byte| matches!(byte, b'\'' | b'\"'))
        .ok_or_else(|| CampusServiceError::protocol("itsapp 挑战值缺少引号"))?;
    let subject_start = md5_start + 1;
    let subject_end = tail[subject_start..]
        .find(md5_quote as char)
        .map(|offset| subject_start + offset)
        .ok_or_else(|| CampusServiceError::protocol("itsapp 挑战值不完整"))?;
    let subject = &tail[subject_start..subject_end];
    let address = subject
        .parse::<IpAddr>()
        .map_err(|_| CampusServiceError::protocol("itsapp 挑战值不是 IP"))?;
    if !matches!(address, IpAddr::V4(Ipv4Addr { .. })) {
        return Err(CampusServiceError::protocol("itsapp 挑战值不是 IPv4"));
    }
    let mut target = current
        .join(&literal)
        .map_err(|_| CampusServiceError::protocol("itsapp 导航地址无效"))?;
    validate_url(&target, &[ITS_HOST])?;
    if target.path() != current.path() {
        return Err(CampusServiceError::protocol("itsapp 挑战目标路径发生变化"));
    }
    let pairs = target
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    let dict_keys = pairs
        .iter()
        .filter(|(key, _)| is_its_dict_key(key))
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    if dict_keys.len() != 1 {
        return Err(CampusServiceError::protocol("itsapp dictkey 不唯一"));
    }
    let dict_key = &dict_keys[0];
    let mut target_values = unique_query_values(&target)?;
    if target_values.remove(dict_key).as_deref() != Some("") {
        return Err(CampusServiceError::protocol("itsapp dictkey 已包含未知值"));
    }
    match current.path() {
        "/uc/api/oauth/index" => validate_its_oauth_values(&target_values)?,
        "/a_bjut/api/sso/index" => {
            let current_values = unique_query_values(current)?;
            if target_values != current_values
                || current_values.len() != 3
                || current_values.get("from").map(String::as_str) != Some("wap")
                || current_values.get("ticket").is_none_or(String::is_empty)
            {
                return Err(CampusServiceError::protocol("itsapp SSO 挑战参数发生变化"));
            }
            let oauth = parse_url(
                current_values
                    .get("redirect")
                    .ok_or_else(|| CampusServiceError::protocol("itsapp SSO 缺少回跳地址"))?,
            )?;
            validate_url(&oauth, &[ITS_HOST])?;
            if oauth.path() != "/uc/api/oauth/index" {
                return Err(CampusServiceError::protocol(
                    "itsapp SSO 没有回到 OAuth 入口",
                ));
            }
            validate_its_oauth_values(&unique_query_values(&oauth)?)?;
        }
        _ => {
            return Err(CampusServiceError::protocol("itsapp 挑战出现在未知路径"));
        }
    }
    let digest = format!("{:x}", md5::compute(subject.as_bytes()));
    target.set_query(None);
    {
        let mut query = target.query_pairs_mut();
        for (key, value) in pairs {
            query.append_pair(&key, if key == *dict_key { &digest } else { &value });
        }
    }
    Ok(target)
}

fn is_its_dict_key(key: &str) -> bool {
    key.strip_suffix("dictkey").is_some_and(|prefix| {
        (8..=64).contains(&prefix.len()) && prefix.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}

fn unique_query_values(url: &Url) -> Result<BTreeMap<String, String>, CampusServiceError> {
    let mut values = BTreeMap::new();
    for (key, value) in url.query_pairs() {
        if values
            .insert(key.into_owned(), value.into_owned())
            .is_some()
        {
            return Err(CampusServiceError::protocol("itsapp 导航参数重复"));
        }
    }
    Ok(values)
}

fn validate_its_oauth_values(values: &BTreeMap<String, String>) -> Result<(), CampusServiceError> {
    if values.len() != 4
        || values.get("redirect").map(String::as_str) != Some(YD_ENTRY)
        || values.get("appid").map(String::as_str) != Some(YD_APP_ID)
        || values.get("state").map(String::as_str) != Some("V8YKT")
        || values.get("qrcode").map(String::as_str) != Some("1")
    {
        return Err(CampusServiceError::protocol("itsapp OAuth 参数发生变化"));
    }
    Ok(())
}

fn redirect_target(
    current: &Url,
    response: &Response,
    allowed_hosts: &[&str],
) -> Result<Url, CampusServiceError> {
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| CampusServiceError::protocol("重定向缺少 Location"))?;
    let next = current
        .join(location)
        .map_err(|_| CampusServiceError::protocol("重定向地址无效"))?;
    validate_url(&next, allowed_hosts)?;
    Ok(next)
}

fn validate_url(url: &Url, allowed_hosts: &[&str]) -> Result<(), CampusServiceError> {
    if url.scheme() != "https"
        || !allowed_hosts.contains(&url.host_str().unwrap_or_default())
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(CampusServiceError::protocol("请求离开受信任的 HTTPS 域名"));
    }
    Ok(())
}

fn parse_url(value: &str) -> Result<Url, CampusServiceError> {
    Url::parse(value).map_err(|_| CampusServiceError::protocol("校园服务地址无效"))
}

fn extract_openid(url: &Url) -> Result<String, CampusServiceError> {
    let query = url
        .fragment()
        .and_then(|fragment| fragment.split_once('?').map(|(_, query)| query))
        .ok_or_else(|| CampusServiceError::protocol("移动门户地址缺少 openid"))?;
    let dummy = Url::parse(&format!("https://placeholder.invalid/?{query}"))
        .map_err(|_| CampusServiceError::protocol("移动门户 openid 参数无效"))?;
    let values = dummy
        .query_pairs()
        .filter(|(key, _)| key == "openid")
        .map(|(_, value)| value.into_owned())
        .collect::<Vec<_>>();
    if values.len() != 1
        || !(8..=128).contains(&values[0].len())
        || !values[0]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._~-".contains(&byte))
    {
        return Err(CampusServiceError::protocol("移动门户 openid 格式无效"));
    }
    Ok(values[0].clone())
}

fn ensure_code_zero(value: &Value, operation: &str) -> Result<(), CampusServiceError> {
    if value.get("code").and_then(Value::as_i64) == Some(0) {
        Ok(())
    } else {
        Err(CampusServiceError::rejected(format!(
            "{operation}失败：{}",
            response_message(value)
        )))
    }
}

fn ensure_success(value: &Value, operation: &str) -> Result<(), CampusServiceError> {
    if value.get("success").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(CampusServiceError::rejected(format!(
            "{operation}失败：{}",
            response_message(value)
        )))
    }
}

fn response_message(value: &Value) -> String {
    for key in ["message", "msg", "title"] {
        if let Some(message) = value.get(key).and_then(Value::as_str) {
            if !message.trim().is_empty() {
                return message.trim().to_string();
            }
        }
    }
    value
        .get("code")
        .map(|code| format!("服务器返回代码 {code}"))
        .unwrap_or_else(|| "服务器未提供原因".to_string())
}

fn required_json_string(value: Option<&Value>, field: &str) -> Result<String, CampusServiceError> {
    value
        .and_then(|value| match value {
            Value::String(value) => Some(value.trim().to_string()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CampusServiceError::protocol(format!("接口缺少 {field}")))
}

fn validate_new_password(new: &str, old: &str) -> Result<(), CampusServiceError> {
    if new == old {
        return Err(CampusServiceError::rejected("新密码不能与当前密码相同"));
    }
    if !(12..=16).contains(&new.len())
        || !new
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!@#$%^&*()".contains(&byte))
        || !new.bytes().any(|byte| byte.is_ascii_lowercase())
        || !new.bytes().any(|byte| byte.is_ascii_uppercase())
        || !new.bytes().any(|byte| byte.is_ascii_digit())
        || !new.bytes().any(|byte| b"!@#$%^&*()".contains(&byte))
    {
        return Err(CampusServiceError::rejected(
            "新密码须为 12–16 位，并同时包含大写字母、小写字母、数字和 !@#$%^&*() 中的特殊字符",
        ));
    }
    Ok(())
}

fn validate_target_account(value: &str) -> Result<String, CampusServiceError> {
    let value = value.trim();
    if !(5..=20).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(CampusServiceError::rejected("请输入 5–20 位有效学工号"));
    }
    Ok(value.to_string())
}

fn canonical_amount(value: &str) -> Result<String, CampusServiceError> {
    let value = value.trim();
    let mut split = value.split('.');
    let whole = split.next().unwrap_or_default();
    let fraction = split.next();
    if split.next().is_some()
        || whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.is_some_and(|part| {
            part.is_empty() || part.len() > 2 || !part.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(CampusServiceError::rejected("充值金额最多保留两位小数"));
    }
    let whole_value = whole
        .parse::<u32>()
        .map_err(|_| CampusServiceError::rejected("充值金额无效"))?;
    let fraction_value = match fraction.unwrap_or_default() {
        "" => 0,
        one if one.len() == 1 => one.parse::<u32>().unwrap_or(0) * 10,
        two => two.parse::<u32>().unwrap_or(0),
    };
    let cents = whole_value
        .checked_mul(100)
        .and_then(|value| value.checked_add(fraction_value))
        .ok_or_else(|| CampusServiceError::rejected("充值金额过大"))?;
    if cents == 0 || cents > 50_000 {
        return Err(CampusServiceError::rejected(
            "充值金额必须大于 0 且不超过 500 元",
        ));
    }
    Ok(if cents % 100 == 0 {
        (cents / 100).to_string()
    } else {
        format!("{}.{:02}", cents / 100, cents % 100)
    })
}

fn parse_money_cents(value: &str) -> Option<u64> {
    let normalized = trim_currency(value);
    let mut split = normalized.split('.');
    let whole = split.next()?;
    let fraction = split.next().unwrap_or_default();
    if split.next().is_some()
        || whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.len() > 2
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let whole = whole.parse::<u64>().ok()?;
    let fraction = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<u64>().ok()? * 10,
        _ => fraction.parse::<u64>().ok()?,
    };
    whole.checked_mul(100)?.checked_add(fraction)
}

fn trim_currency(value: &str) -> String {
    value.trim().trim_end_matches('元').trim().to_string()
}

fn format_balance_for_trade(value: &str) -> String {
    let value = trim_currency(value);
    if value.is_empty() {
        value
    } else {
        format!("{value}元")
    }
}

fn confirmation_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = CONFIRMATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:032x}-{sequence:016x}")
}

fn tags<'a>(source: &'a str, name: &str) -> Vec<&'a str> {
    let lower = source.to_ascii_lowercase();
    let needle = format!("<{name}");
    let mut result = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative) = lower[cursor..].find(&needle) {
        let start = cursor + relative;
        let Some(end_relative) = source[start..].find('>') else {
            break;
        };
        let end = start + end_relative + 1;
        result.push(&source[start..end]);
        cursor = end;
    }
    result
}

fn attribute(tag: &str, wanted: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut index = 1usize;
    while index < bytes.len() {
        while bytes
            .get(index)
            .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'<' | b'/' | b'>'))
        {
            index += 1;
        }
        let start = index;
        while bytes
            .get(index)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
        {
            index += 1;
        }
        if start == index {
            index += 1;
            continue;
        }
        let name = &tag[start..index];
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if bytes.get(index) != Some(&b'=') {
            if name.eq_ignore_ascii_case(wanted) {
                return Some(String::new());
            }
            continue;
        }
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        let quote = bytes
            .get(index)
            .copied()
            .filter(|byte| matches!(byte, b'\'' | b'\"'));
        if quote.is_some() {
            index += 1;
        }
        let value_start = index;
        while let Some(byte) = bytes.get(index).copied() {
            if quote
                .map(|active| byte == active)
                .unwrap_or_else(|| byte.is_ascii_whitespace() || byte == b'>')
            {
                break;
            }
            index += 1;
        }
        if name.eq_ignore_ascii_case(wanted) {
            return Some(tag[value_start..index].to_string());
        }
        if quote.is_some() && bytes.get(index).is_some() {
            index += 1;
        }
    }
    None
}

fn decode_html_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alipay_form(app_id: &str) -> String {
        format!(
            r#"<html><body><form method="post" action="https://openapi.alipay.com/gateway.do?alipay_sdk=alipay-sdk-java-dynamicVersionNo&amp;app_id={app_id}&amp;charset=UTF-8&amp;format=json&amp;method=alipay.trade.wap.pay&amp;notify_url=https%3A%2F%2Falipay.bjut.edu.cn%2FPayPreService%2FaliPayWapBackResNotify&amp;return_url=https%3A%2F%2Falipay.bjut.edu.cn%2FPayPreService%2FaliPayWapBackResReturn&amp;sign=abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMN&amp;sign_type=RSA2&amp;timestamp=2026-01-01+00%3A00%3A00&amp;version=1.0"><input type="hidden" name="biz_content" value="{{&quot;out_trade_no&quot;:&quot;12345678901234567890&quot;,&quot;total_amount&quot;:&quot;0.01&quot;,&quot;subject&quot;:&quot;支付宝充值校园卡&quot;,&quot;product_code&quot;:&quot;QUICK_WAP_WAY&quot;}}"></form></body></html>"#
        )
    }

    #[test]
    fn builds_gettable_alipay_gateway_url_from_captured_form_shape() {
        let url = alipay_gateway_url(&alipay_form(ALIPAY_APP_ID), "0.01").unwrap();
        let parsed = Url::parse(&url).unwrap();
        assert_eq!(parsed.host_str(), Some(ALIPAY_GATEWAY_HOST));
        assert_eq!(parsed.path(), "/gateway.do");
        let values = unique_query_values(&parsed).unwrap();
        assert_eq!(
            values.get("method").map(String::as_str),
            Some("alipay.trade.wap.pay")
        );
        assert!(values
            .get("biz_content")
            .is_some_and(|value| value.contains("支付宝充值校园卡")));
    }

    #[test]
    fn rejects_alipay_form_for_a_different_merchant() {
        assert!(alipay_gateway_url(&alipay_form("0000000000000000"), "0.01").is_err());
    }

    #[test]
    fn validates_password_policy_from_captured_uc_rule() {
        assert!(validate_new_password("Abcdef12345!", "old").is_ok());
        assert!(validate_new_password("abcdef12345!", "old").is_err());
        assert!(validate_new_password("Abcdef123456", "old").is_err());
        assert!(validate_new_password("Abc123!", "old").is_err());
    }

    #[test]
    fn canonicalizes_recharge_amount_without_rounding() {
        assert_eq!(canonical_amount("10").unwrap(), "10");
        assert_eq!(canonical_amount("10.5").unwrap(), "10.50");
        assert_eq!(canonical_amount("0.01").unwrap(), "0.01");
        assert!(canonical_amount("0").is_err());
        assert!(canonical_amount("500.01").is_err());
        assert!(canonical_amount("1.001").is_err());
        assert_eq!(parse_money_cents("1200.50 元"), Some(120_050));
        assert_eq!(parse_money_cents("0"), Some(0));
    }

    #[test]
    fn cookie_jar_obeys_domain_and_path_boundaries() {
        let mut jar = DomainCookies::default();
        let url = Url::parse("https://cas.bjut.edu.cn/login").unwrap();
        let mut headers = HeaderMap::new();
        headers.append(SET_COOKIE, "CASTGC=secret; Path=/; Secure".parse().unwrap());
        headers.append(
            SET_COOKIE,
            "shared=value; Domain=.bjut.edu.cn; Path=/api"
                .parse()
                .unwrap(),
        );
        jar.absorb(&url, &headers);
        assert_eq!(
            jar.header(&Url::parse("https://cas.bjut.edu.cn/login").unwrap())
                .as_deref(),
            Some("CASTGC=secret")
        );
        assert_eq!(
            jar.header(&Url::parse("https://uc.bjut.edu.cn/api/status").unwrap())
                .as_deref(),
            Some("shared=value")
        );
        assert!(jar
            .header(&Url::parse("https://uc.bjut.edu.cn/application").unwrap())
            .is_none());
    }

    #[test]
    fn parses_and_validates_cas_login_form() {
        let page = r#"<form method="post" action="">
          <input type="hidden" name="execution" value="opaque-token">
          <input name="username"><input type="password" name="password">
        </form>"#;
        let url = Url::parse(
            "https://cas.bjut.edu.cn/login?service=https%3A%2F%2Fuc.bjut.edu.cn%2Fapi%2Flogin%3Ftarget%3Dhttps%253A%252F%252Fuc.bjut.edu.cn%252F%2523%252Fuser%252Flogin",
        )
        .unwrap();
        let (_, fields) = cas_login_form(page, &url, "student", "secret").unwrap();
        let fields = fields.into_iter().collect::<BTreeMap<_, _>>();
        assert_eq!(
            fields.get("execution").map(String::as_str),
            Some("opaque-token")
        );
        assert_eq!(fields.get("username").map(String::as_str), Some("student"));
        assert_eq!(fields.get("password").map(String::as_str), Some("secret"));
        assert_eq!(request_stage(&url), "打开 CAS 登录页");
        assert!(validate_cas_action(&Url::parse(CAS_LOGIN_ENTRY).unwrap()).is_ok());
    }

    #[test]
    fn both_recharge_routes_use_the_current_api() {
        for route in [
            "/pages/recharge/networkFeeCharge/networkFeeCharge",
            "/pages/recharge/networkFeeCharge/networkFeeChargeNew",
        ] {
            let api = RechargeApi::from_route(route).unwrap();
            assert_eq!(api.target_query_path, "/channel/get085Detail");
            assert_eq!(api.factory_code, "N006");
        }
    }

    #[test]
    fn solves_itsapp_md5_challenge_with_spacing() {
        let current = Url::parse(
            "https://itsapp.bjut.edu.cn/uc/api/oauth/index?redirect=https%3A%2F%2Fydapp.bjut.edu.cn%2FopenV8HomePage&appid=200220816093810809&state=V8YKT&qrcode=1",
        )
        .unwrap();
        let html = r#"<script>
          window.location.href = "?redirect=https%3A%2F%2Fydapp.bjut.edu.cn%2FopenV8HomePage&appid=200220816093810809&state=V8YKT&qrcode=1&abcdefghijklmnopdictkey=" + md5("192.0.2.8");
        </script>"#;
        let solved = solve_its_challenge(&current, html).unwrap();
        let pairs = solved.query_pairs().collect::<BTreeMap<_, _>>();
        let expected_digest = format!("{:x}", md5::compute(b"192.0.2.8"));
        assert_eq!(
            pairs.get("redirect").map(|value| value.as_ref()),
            Some(YD_ENTRY)
        );
        assert_eq!(
            pairs
                .get("abcdefghijklmnopdictkey")
                .map(|value| value.as_ref()),
            Some(expected_digest.as_str())
        );
    }

    #[test]
    fn solves_itsapp_sso_md5_challenge_from_browser_flow() {
        let oauth = Url::parse(
            "https://itsapp.bjut.edu.cn/uc/api/oauth/index?redirect=https%3A%2F%2Fydapp.bjut.edu.cn%2FopenV8HomePage&appid=200220816093810809&state=V8YKT&qrcode=1",
        )
        .unwrap();
        let mut current = Url::parse("https://itsapp.bjut.edu.cn/a_bjut/api/sso/index").unwrap();
        current
            .query_pairs_mut()
            .append_pair("redirect", oauth.as_str())
            .append_pair("from", "wap")
            .append_pair("ticket", "ST-redacted-test-ticket");
        let literal = format!(
            "?{}&a17f5f4fdictkey=",
            current.query().expect("test URL has a query")
        );
        let html = format!(
            r#"<script src="/a155a53cde0f5585235d18ab56219d67.js"></script>
              <script>window.location.href="{literal}"+md5("192.0.2.9");</script>"#
        );

        let solved = solve_its_challenge(&current, &html).unwrap();
        let pairs = solved.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(
            pairs.get("ticket").map(|value| value.as_ref()),
            Some("ST-redacted-test-ticket")
        );
        assert_eq!(pairs.get("from").map(|value| value.as_ref()), Some("wap"));
        assert_eq!(
            pairs.get("a17f5f4fdictkey").map(|value| value.as_ref()),
            Some(format!("{:x}", md5::compute(b"192.0.2.9")).as_str())
        );
    }
}
