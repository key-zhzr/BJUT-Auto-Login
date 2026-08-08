use super::{
    query_campus_dns_ipv4, redact_request_error, route_source_ipv4, usable_physical_ipv4,
    VpnCompatibility, LGN6_HOST, LGN_HOST, WLGN_HOST,
};
use crate::network_trust::{campus_wifi_kind, CampusWifiKind};
use reqwest::header::{ACCEPT, CACHE_CONTROL, REFERER};
use reqwest::{Client, Url};
use serde_json::Value;
use std::fmt::Write as _;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) const AMBIGUOUS_LOGIN_RESULT: &str = "认证结果暂无法确认";

const DORM_HTTP_LOGIN: &str = "http://10.21.221.98:801/eportal/portal/login";
const DORM_HTTPS_LOGIN: &str = "https://10.21.221.98:802/eportal/portal/login";
const DORM_HTTP_REFERER: &str = "http://10.21.221.98/";
const DORM_HTTPS_REFERER: &str = "https://10.21.221.98/";
const WIFI_HTTP_LOGIN: &str = "http://10.21.251.3/drcom/login";
const WIFI_HTTPS_LOGIN: &str = "https://wlgn.bjut.edu.cn/drcom/login";
const WIFI_HTTP_REFERER: &str = "http://10.21.251.3/";
const WIFI_HTTPS_REFERER: &str = "https://wlgn.bjut.edu.cn/";
const LGN_REFERER: &str = "https://lgn.bjut.edu.cn/";
const LGN_PROGRAM_INDEX: &str = "o4OBee1755497815";
const LGN_PAGE_INDEX: &str = "cHAmjX1755497856";
const LGN_JS_VERSION: &str = "4.2.2";
const EPORTAL_XOR_KEY: u16 = 0x16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoginType {
    Type1,
    Type2,
    Type3,
    Unknown,
}

impl LoginType {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Type1 => "Type1_221_98",
            Self::Type2 => "Type2_251_3",
            Self::Type3 => "Type3_172_30",
            Self::Unknown => "Unknown",
        }
    }
}

fn random_request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:04}", nanos % 10_000)
}

fn login_type_hint(ssid: &str) -> Option<LoginType> {
    match campus_wifi_kind(ssid) {
        Some(CampusWifiKind::Dormitory) => Some(LoginType::Type1),
        Some(CampusWifiKind::Public) => Some(LoginType::Type2),
        None => None,
    }
}

fn login_probe_candidates(ssid: &str, transport: &str) -> Vec<LoginType> {
    let mut candidates = if transport.eq_ignore_ascii_case("wifi") {
        vec![LoginType::Type1, LoginType::Type2]
    } else if transport.eq_ignore_ascii_case("ethernet") {
        vec![LoginType::Type3]
    } else {
        vec![LoginType::Type1, LoginType::Type2, LoginType::Type3]
    };

    // SSID is only a priority hint. A matching name must never bypass the
    // protocol-specific response probe, because SSIDs can be renamed or
    // spoofed. Ethernet also deliberately ignores Wi-Fi naming hints.
    if let Some(hint) = login_type_hint(ssid) {
        if let Some(position) = candidates.iter().position(|candidate| *candidate == hint) {
            candidates.swap(0, position);
        }
    }
    candidates
}

pub(crate) async fn portal_client(
    compatibility: VpnCompatibility,
    login_type: &LoginType,
    timeout: Duration,
) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .use_rustls_tls();
    let hosts: Vec<(&str, Vec<Ipv4Addr>)> = match login_type {
        LoginType::Type2 => vec![(WLGN_HOST, vec![Ipv4Addr::new(10, 21, 251, 3)])],
        LoginType::Type3 => vec![
            (
                LGN_HOST,
                vec![
                    Ipv4Addr::new(172, 30, 201, 2),
                    Ipv4Addr::new(172, 30, 201, 10),
                ],
            ),
            (
                LGN6_HOST,
                vec![
                    Ipv4Addr::new(172, 30, 201, 2),
                    Ipv4Addr::new(172, 30, 201, 10),
                ],
            ),
        ],
        _ => Vec::new(),
    };
    if matches!(
        compatibility,
        VpnCompatibility::Low | VpnCompatibility::High
    ) {
        for (host, fixed_addresses) in hosts {
            let addresses = if compatibility == VpnCompatibility::Low {
                let host_owned = host.to_string();
                tokio::task::spawn_blocking(move || query_campus_dns_ipv4(&host_owned))
                    .await
                    .map_err(|error| format!("校园网 DNS 任务失败：{error}"))??
            } else {
                fixed_addresses
            };
            let socket_addresses: Vec<SocketAddr> = addresses
                .into_iter()
                .map(|address| SocketAddr::new(IpAddr::V4(address), 443))
                .collect();
            builder = builder.resolve_to_addrs(host, &socket_addresses);
        }
    }
    builder.build().map_err(redact_request_error)
}

pub(crate) fn portal_probe_urls(
    compatibility: VpnCompatibility,
    login_type: &LoginType,
) -> Vec<String> {
    match login_type {
        LoginType::Type1 if compatibility == VpnCompatibility::Maximum => {
            vec![DORM_HTTP_LOGIN.to_string()]
        }
        LoginType::Type1 => vec![DORM_HTTPS_LOGIN.to_string()],
        LoginType::Type2 if compatibility == VpnCompatibility::Maximum => {
            vec![WIFI_HTTP_LOGIN.to_string()]
        }
        LoginType::Type2 => vec![WIFI_HTTPS_LOGIN.to_string()],
        LoginType::Type3 if compatibility == VpnCompatibility::Maximum => {
            let primary = lgn_user_info_url(compatibility);
            let secondary = primary.replacen("172.30.201.2", "172.30.201.10", 1);
            vec![primary, secondary]
        }
        LoginType::Type3 => {
            let mut urls = vec![lgn_user_info_url(compatibility)];
            if let Ok(ipv6_url) = lgn_observed_ipv6_url() {
                urls.push(ipv6_url.to_string());
            }
            urls
        }
        LoginType::Unknown => Vec::new(),
    }
}

fn login_readiness_probe_urls(
    compatibility: VpnCompatibility,
    login_type: &LoginType,
) -> Vec<String> {
    let mut urls = portal_probe_urls(compatibility, login_type);
    if *login_type == LoginType::Type3 && !urls.iter().any(|url| url.contains("/drcom/getipv6")) {
        if let Ok(ipv6_url) = lgn_observed_ipv6_url() {
            urls.push(ipv6_url.to_string());
        }
    }
    urls
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortalProbeResult {
    NotDetected,
    PortalDetected,
    LoginReady,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoginTypeDetection {
    pub(crate) login_type: LoginType,
    pub(crate) portal_detected: bool,
    pub(crate) login_ready: bool,
}

impl LoginTypeDetection {
    fn not_detected() -> Self {
        Self {
            login_type: LoginType::Unknown,
            portal_detected: false,
            login_ready: false,
        }
    }

    fn from_probe(login_type: LoginType, result: PortalProbeResult) -> Self {
        Self {
            login_type,
            portal_detected: result != PortalProbeResult::NotDetected,
            login_ready: result == PortalProbeResult::LoginReady,
        }
    }

    fn into_ready_login_type(self) -> LoginType {
        if self.login_ready {
            self.login_type
        } else {
            LoginType::Unknown
        }
    }
}

#[derive(Debug, Default)]
struct Type3ProbeEvidence {
    portal_detected: bool,
    ipv6_login_ready: bool,
}

impl Type3ProbeEvidence {
    fn record(&mut self, url: &str, body: &str) {
        if body.trim().is_empty() {
            return;
        }
        if url.contains("loadUserInfo") {
            self.portal_detected |= probe_body_matches(&LoginType::Type3, url, body);
            return;
        }
        if url.contains("/drcom/getipv6") {
            // A syntactically valid getipv6 response proves that the Type 3
            // portal is present. It is only safe to start the two-stack login
            // flow when that response also contains a usable IPv6 address.
            self.portal_detected |=
                jsonp_object(body).is_some_and(|value| value.get("result").is_some());
            self.ipv6_login_ready |= parse_observed_ip(body, true).is_ok();
        }
    }

    fn result(&self) -> PortalProbeResult {
        if self.ipv6_login_ready {
            PortalProbeResult::LoginReady
        } else if self.portal_detected {
            PortalProbeResult::PortalDetected
        } else {
            PortalProbeResult::NotDetected
        }
    }
}

async fn probe_login_type(
    compatibility: VpnCompatibility,
    login_type: LoginType,
) -> PortalProbeResult {
    // Type 3 maximum compatibility still needs the TLS/SNI-preserving lgn6
    // endpoint to prove that IPv6 login is actually ready.
    let client_compatibility =
        if login_type == LoginType::Type3 && compatibility == VpnCompatibility::Maximum {
            VpnCompatibility::High
        } else {
            compatibility
        };
    let Ok(client) = portal_client(
        client_compatibility,
        &login_type,
        Duration::from_millis(1800),
    )
    .await
    else {
        return PortalProbeResult::NotDetected;
    };
    let mut type3_evidence = Type3ProbeEvidence::default();
    for url in login_readiness_probe_urls(compatibility, &login_type) {
        let Ok(response) = client
            .get(&url)
            .header("Cache-Control", "no-cache, no-store")
            .send()
            .await
        else {
            continue;
        };
        // Reachability of the fixed campus endpoint is the only response
        // characteristic currently documented for probes. Reject redirects,
        // server errors and empty responses rather than treating any completed
        // TCP/TLS request as a confirmed protocol.
        let status_ok = response.status().is_success();
        let body = response.text().await.unwrap_or_default();
        if !status_ok {
            continue;
        }
        if login_type == LoginType::Type3 {
            type3_evidence.record(&url, &body);
        } else if probe_body_matches(&login_type, &url, &body) {
            return PortalProbeResult::LoginReady;
        }
    }
    if login_type == LoginType::Type3 {
        type3_evidence.result()
    } else {
        PortalProbeResult::NotDetected
    }
}

fn jsonp_object(text: &str) -> Option<Value> {
    let start = text.find('(')?;
    let end = text.rfind(')').filter(|end| *end > start)?;
    serde_json::from_str(&text[start + 1..end]).ok()
}

fn probe_body_matches(login_type: &LoginType, url: &str, body: &str) -> bool {
    if body.trim().is_empty() {
        return false;
    }
    let normalized = body.to_ascii_lowercase();
    match login_type {
        LoginType::Type1 => {
            normalized.contains("eportal")
                || normalized.contains("user_account")
                || jsonp_object(body).is_some_and(|value| value.get("result").is_some())
        }
        LoginType::Type2 => {
            normalized.contains("drcom")
                || normalized.contains("ddddd")
                || normalized.contains("0mkkey")
                || jsonp_object(body).is_some_and(|value| value.get("result").is_some())
        }
        LoginType::Type3 if url.contains("loadUserInfo") => {
            jsonp_object(body).is_some_and(|value| {
                value.get("code").is_some() && value.get("user_info").is_some_and(Value::is_object)
            })
        }
        LoginType::Type3 if url.contains("/drcom/getipv6") => parse_observed_ip(body, true).is_ok(),
        LoginType::Type3 => {
            normalized.contains("name=\"v6ip\"")
                || normalized.contains("name='v6ip'")
                || normalized.contains("lgn.bjut.edu.cn")
        }
        LoginType::Unknown => false,
    }
}

pub(crate) async fn detect_login_type_details_rust(
    compatibility: VpnCompatibility,
    ssid: &str,
    transport: &str,
) -> LoginTypeDetection {
    let mut portal_only = None;
    for candidate in login_probe_candidates(ssid, transport) {
        let result = probe_login_type(compatibility, candidate.clone()).await;
        if result == PortalProbeResult::LoginReady {
            return LoginTypeDetection::from_probe(candidate, result);
        }
        if result == PortalProbeResult::PortalDetected && portal_only.is_none() {
            portal_only = Some(LoginTypeDetection::from_probe(candidate, result));
        }
    }
    portal_only.unwrap_or_else(LoginTypeDetection::not_detected)
}

pub(crate) async fn detect_login_type_rust(
    compatibility: VpnCompatibility,
    ssid: &str,
    transport: &str,
) -> LoginType {
    detect_login_type_details_rust(compatibility, ssid, transport)
        .await
        .into_ready_login_type()
}

fn parse_dr_response(text: &str) -> Result<(bool, String), String> {
    let start = text
        .find('(')
        .ok_or_else(|| format!("{AMBIGUOUS_LOGIN_RESULT}：登录网关未返回预期 JSONP 响应"))?;
    let end = text
        .rfind(')')
        .filter(|end| *end > start)
        .ok_or_else(|| format!("{AMBIGUOUS_LOGIN_RESULT}：登录网关返回的 JSONP 不完整"))?;
    let data: Value = serde_json::from_str(&text[start + 1..end])
        .map_err(|_| format!("{AMBIGUOUS_LOGIN_RESULT}：登录网关响应无法解析"))?;
    let result = data
        .get("result")
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str()?.parse::<i64>().ok())
        })
        .ok_or_else(|| format!("{AMBIGUOUS_LOGIN_RESULT}：登录网关未返回 result 字段"))?;
    if !matches!(result, 0 | 1) {
        return Err(format!(
            "{AMBIGUOUS_LOGIN_RESULT}：登录网关返回了未知 result 值 {result}"
        ));
    }
    let message = data
        .get("msga")
        .or_else(|| data.get("msg"))
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string())
        })
        .unwrap_or_else(|| {
            if result == 1 {
                "Portal协议认证成功！".to_string()
            } else {
                "登录被认证网关拒绝".to_string()
            }
        });
    Ok((result == 1, message))
}

fn eportal_encrypt(value: &str) -> String {
    let mut encrypted = String::with_capacity(value.len() * 2);
    for unit in value.encode_utf16() {
        let _ = write!(encrypted, "{:02x}", unit ^ EPORTAL_XOR_KEY);
    }
    encrypted
}

fn parse_observed_ip(text: &str, expect_ipv6: bool) -> Result<String, String> {
    let data =
        jsonp_object(text).ok_or_else(|| "有线登录地址发现接口未返回预期 JSONP".to_string())?;
    let result = data
        .get("result")
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str()?.parse::<i64>().ok())
        })
        .unwrap_or_default();
    if result != 1 {
        return Err("有线登录地址发现接口未返回成功结果".to_string());
    }
    let value = data
        .get("ip")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "有线登录地址发现接口未返回 IP 地址".to_string())?;
    let address = value
        .parse::<IpAddr>()
        .map_err(|_| "有线登录地址发现接口返回了无效 IP 地址".to_string())?;
    if address.is_ipv6() != expect_ipv6 {
        return Err(if expect_ipv6 {
            "有线登录地址发现接口未返回 IPv6 地址".to_string()
        } else {
            "有线登录地址发现接口未返回 IPv4 地址".to_string()
        });
    }
    Ok(address.to_string())
}

fn lgn_observed_ipv6_url() -> Result<Url, String> {
    let mut url =
        Url::parse("https://lgn6.bjut.edu.cn/drcom/getipv6").map_err(|error| error.to_string())?;
    url.query_pairs_mut()
        .append_pair("callback", "dr1004")
        .append_pair("program_index", LGN_PROGRAM_INDEX)
        .append_pair("page_index", LGN_PAGE_INDEX)
        .append_pair("jsVersion", LGN_JS_VERSION)
        .append_pair("v", &random_request_id())
        .append_pair("lang", "zh");
    Ok(url)
}

fn lgn_login_url(
    user: &str,
    pass: &str,
    local_ipv4: &str,
    observed_ipv6: &str,
) -> Result<Url, String> {
    let mut url = Url::parse("https://lgn.bjut.edu.cn:802/eportal/portal/login")
        .map_err(|error| error.to_string())?;
    let account = if user.starts_with(",0,") {
        user.to_string()
    } else {
        format!(",0,{user}")
    };
    let fields = [
        ("callback", "dr1005".to_string()),
        ("login_method", "1".to_string()),
        ("user_account", account),
        ("user_password", pass.to_string()),
        ("wlan_user_ip", local_ipv4.to_string()),
        ("wlan_user_ipv6", observed_ipv6.to_string()),
        ("wlan_user_mac", "000000000000".to_string()),
        ("wlan_vlan_id", "0".to_string()),
        ("wlan_ac_ip", String::new()),
        ("wlan_ac_name", String::new()),
        ("authex_enable", String::new()),
        ("jsVersion", LGN_JS_VERSION.to_string()),
        ("login_ip_type", "0".to_string()),
        ("terminal_type", "3".to_string()),
        ("lang", "zh-cn".to_string()),
        ("program_index", LGN_PROGRAM_INDEX.to_string()),
        ("page_index", LGN_PAGE_INDEX.to_string()),
    ];
    {
        let mut query = url.query_pairs_mut();
        for (name, value) in fields {
            query.append_pair(name, &eportal_encrypt(&value));
        }
        query
            .append_pair("encrypt", "1")
            .append_pair("v", &random_request_id())
            .append_pair("lang", "zh");
    }
    Ok(url)
}

pub(crate) fn lgn_user_info_url(compatibility: VpnCompatibility) -> String {
    let query = format!(
        "callback=726427262624&lang=6c7e3b7578&program_index=79225954737327212323222f212e2723&page_index=755e577b7c4e27212323222f212e2320&user_account=&wlan_user_ip=&wlan_user_ipv6=&wlan_user_mac=262626262626262626262626&jsVersion=22384e&encrypt=1&v={}&lang=zh",
        random_request_id()
    );
    if compatibility == VpnCompatibility::Maximum {
        format!("http://172.30.201.2:801/eportal/portal/page/loadUserInfo?{query}")
    } else {
        format!("https://lgn.bjut.edu.cn:802/eportal/portal/page/loadUserInfo?{query}")
    }
}

fn login_source_ipv4(
    physical_ipv4: Option<&str>,
    destination: &str,
    network_label: &str,
) -> Result<String, String> {
    if let Some(candidate) = physical_ipv4
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return usable_physical_ipv4(candidate)
            .map(|address| address.to_string())
            .ok_or_else(|| {
                format!("{network_label}登录所需的物理接口 IPv4 无效；已拒绝使用 VPN/TUN Fake-IP")
            });
    }

    let routed = route_source_ipv4(destination);
    usable_physical_ipv4(&routed)
        .map(|address| address.to_string())
        .ok_or_else(|| {
            format!("{network_label}登录前无法确定物理接口 IPv4；请检查 Wi-Fi/有线接口或 VPN 分流")
        })
}

async fn login_lgn_once(
    client: &Client,
    user: &str,
    pass: &str,
    physical_ipv4: Option<&str>,
) -> Result<(bool, String), String> {
    let local_ipv4 = login_source_ipv4(physical_ipv4, "172.30.201.2:802", "lgn 有线")?;

    let ipv6_response = client
        .get(lgn_observed_ipv6_url()?)
        .header(ACCEPT, "*/*")
        .header(REFERER, LGN_REFERER)
        .header(CACHE_CONTROL, "no-cache, no-store")
        .send()
        .await
        .map_err(|error| portal_request_error("lgn IPv6 地址发现", error, None))?;
    if !ipv6_response.status().is_success() {
        return Err(format!(
            "lgn IPv6 地址发现接口返回 HTTP {}",
            ipv6_response.status()
        ));
    }
    let ipv6_body = ipv6_response.text().await.map_err(|error| {
        format!(
            "lgn IPv6 地址发现响应读取失败：{}",
            redact_request_error(error)
        )
    })?;
    let observed_ipv6 = parse_observed_ip(&ipv6_body, true)?;
    let login_url = lgn_login_url(user, pass, &local_ipv4, &observed_ipv6)?;
    get_jsonp_login(client, login_url, LGN_REFERER, "lgn 有线登录", None).await
}

fn type1_login_url(
    compatibility: VpnCompatibility,
    user: &str,
    pass: &str,
    local_ip: &str,
) -> Result<Url, String> {
    let base = if compatibility == VpnCompatibility::Maximum {
        DORM_HTTP_LOGIN
    } else {
        DORM_HTTPS_LOGIN
    };
    let mut url = Url::parse(base).map_err(|error| error.to_string())?;
    let account = if user.to_ascii_lowercase().ends_with("@campus") {
        user.to_string()
    } else {
        format!("{user}@campus")
    };
    url.query_pairs_mut()
        .append_pair("callback", "dr1003")
        .append_pair("login_method", "1")
        .append_pair("user_account", &account)
        .append_pair("user_password", pass)
        .append_pair("wlan_user_ip", local_ip)
        .append_pair("wlan_user_ipv6", "")
        .append_pair("wlan_user_mac", "000000000000")
        .append_pair("wlan_ac_ip", "")
        .append_pair("wlan_ac_name", "")
        .append_pair("jsVersion", "4.2.1")
        .append_pair("terminal_type", "3")
        .append_pair("lang", "zh-cn")
        .append_pair("v", &random_request_id())
        .append_pair("lang", "zh");
    Ok(url)
}

fn type2_login_url(compatibility: VpnCompatibility, user: &str, pass: &str) -> Result<Url, String> {
    let base = if compatibility == VpnCompatibility::Maximum {
        WIFI_HTTP_LOGIN
    } else {
        WIFI_HTTPS_LOGIN
    };
    let mut url = Url::parse(base).map_err(|error| error.to_string())?;
    url.query_pairs_mut()
        .append_pair("callback", "dr1003")
        .append_pair("DDDDD", user)
        .append_pair("upass", pass)
        .append_pair("0MKKey", "123456")
        .append_pair("R1", "0")
        .append_pair("R2", "")
        .append_pair("R3", "0")
        .append_pair("R6", "0")
        .append_pair("para", "00")
        .append_pair("v6ip", "")
        .append_pair("terminal_type", "1")
        .append_pair("lang", "zh-cn")
        .append_pair("jsVersion", "4.1")
        .append_pair("v", &random_request_id())
        .append_pair("lang", "zh");
    Ok(url)
}

fn portal_request_error(label: &str, error: reqwest::Error, hint: Option<&str>) -> String {
    let category = if error.is_timeout() {
        "请求超时"
    } else if error.is_connect() {
        "连接或 TLS 握手失败"
    } else if error.is_request() {
        "请求构造或发送失败"
    } else if error.is_body() || error.is_decode() {
        "响应读取失败"
    } else {
        "请求失败"
    };
    let mut message = format!("{label}：{category}");
    if let Some(hint) = hint {
        message.push('；');
        message.push_str(hint);
    }
    message
}

async fn get_jsonp_login(
    client: &Client,
    url: Url,
    referer: &str,
    label: &str,
    connection_hint: Option<&str>,
) -> Result<(bool, String), String> {
    let response = client
        .get(url)
        .header(ACCEPT, "*/*")
        .header(REFERER, referer)
        .header(CACHE_CONTROL, "no-cache, no-store")
        .send()
        .await
        .map_err(|error| portal_request_error(label, error, connection_hint))?;
    if !response.status().is_success() {
        return Err(format!("{label}网关返回 HTTP {}", response.status()));
    }
    let text = response
        .text()
        .await
        .map_err(|error| format!("{label}响应读取失败：{}", redact_request_error(error)))?;
    parse_dr_response(&text)
}

pub(crate) async fn login_to_campus_network_rust(
    login_type: LoginType,
    user: &str,
    pass: &str,
    compatibility: VpnCompatibility,
    physical_ipv4: Option<&str>,
) -> Result<(bool, String), String> {
    // The captured lgn eportal flow requires both HTTPS hostnames: lgn6 obtains
    // the observed IPv6 address and lgn:802 receives the encrypted login GET.
    // Even in maximum compatibility mode, preserve TLS/SNI and pin the known
    // campus addresses rather than replacing those hostnames with raw IP URLs.
    let client_compatibility =
        if login_type == LoginType::Type3 && compatibility == VpnCompatibility::Maximum {
            VpnCompatibility::High
        } else {
            compatibility
        };
    let client = portal_client(client_compatibility, &login_type, Duration::from_secs(5)).await?;
    match login_type {
        LoginType::Type1 => {
            let destination = if compatibility == VpnCompatibility::Maximum {
                "10.21.221.98:801"
            } else {
                "10.21.221.98:802"
            };
            let local_ip = login_source_ipv4(physical_ipv4, destination, "bjut-sushe")?;
            let (referer, hint) = if compatibility == VpnCompatibility::Maximum {
                (DORM_HTTP_REFERER, None)
            } else {
                (
                    DORM_HTTPS_REFERER,
                    Some(
                        "实测宿舍网入口使用 HTTP:801；确认当前网络可信后，可临时启用“最高兼容”重试",
                    ),
                )
            };
            get_jsonp_login(
                &client,
                type1_login_url(compatibility, user, pass, &local_ip)?,
                referer,
                "bjut-sushe 登录",
                hint,
            )
            .await
        }
        LoginType::Type2 => {
            let referer = if compatibility == VpnCompatibility::Maximum {
                WIFI_HTTP_REFERER
            } else {
                WIFI_HTTPS_REFERER
            };
            get_jsonp_login(
                &client,
                type2_login_url(compatibility, user, pass)?,
                referer,
                "bjut_wifi 登录",
                None,
            )
            .await
        }
        LoginType::Type3 => login_lgn_once(&client, user, pass, physical_ipv4).await,
        LoginType::Unknown => Err("未设定的登录类型".to_string()),
    }
}

pub(crate) fn login_result_is_ambiguous(error: &str) -> bool {
    error.starts_with(AMBIGUOUS_LOGIN_RESULT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantically_valid_jsonp_results_are_required() {
        assert_eq!(
            parse_dr_response(r#"dr1003({"result":1,"msg":"ok"});"#).unwrap(),
            (true, "ok".to_string())
        );
        assert_eq!(
            parse_dr_response(r#"dr1002({"result":0,"msga":"密码错误"});"#).unwrap(),
            (false, "密码错误".to_string())
        );
        assert!(parse_dr_response("not json").is_err());
        assert!(parse_dr_response(r#"dr1003({"result":2,"msg":"unknown"});"#).is_err());
    }

    #[test]
    fn login_urls_use_the_documented_ports_and_fields() {
        let dorm =
            type1_login_url(VpnCompatibility::Maximum, "25000000", "p!", "10.126.21.113").unwrap();
        assert_eq!(dorm.port(), Some(801));
        assert_eq!(dorm.scheme(), "http");
        assert!(dorm.as_str().contains("terminal_type=3"));
        assert!(dorm.as_str().contains("wlan_user_ip=10.126.21.113"));
        assert!(dorm.as_str().contains("user_account=25000000%40campus"));
        assert_eq!(
            dorm.query_pairs()
                .filter(|(name, _)| name == "lang")
                .count(),
            2
        );

        let wifi = type2_login_url(VpnCompatibility::High, "25000000", "p!").unwrap();
        assert_eq!(wifi.scheme(), "https");
        assert_eq!(wifi.host_str(), Some(WLGN_HOST));
        assert!(wifi.as_str().contains("callback=dr1003"));
        assert_eq!(
            wifi.query_pairs()
                .filter(|(name, _)| name == "lang")
                .count(),
            2
        );
    }

    #[test]
    fn login_source_uses_the_physical_interface_and_rejects_tun_fake_ip() {
        assert_eq!(
            login_source_ipv4(Some("10.3.219.173"), "10.21.221.98:801", "fixture").unwrap(),
            "10.3.219.173"
        );
        assert!(
            login_source_ipv4(Some("198.18.12.34"), "10.21.221.98:801", "fixture")
                .unwrap_err()
                .contains("Fake-IP")
        );
    }

    #[test]
    fn lgn_urls_match_the_encrypted_eportal_flow() {
        assert_eq!(eportal_encrypt("dr1005"), "726427262623");
        assert_eq!(eportal_encrypt(LGN_JS_VERSION), "2238243824");

        let discovery = lgn_observed_ipv6_url().unwrap();
        assert_eq!(
            discovery.as_str().split('?').next().unwrap(),
            "https://lgn6.bjut.edu.cn/drcom/getipv6"
        );
        assert!(discovery.as_str().contains("callback=dr1004"));
        assert!(discovery.as_str().contains("jsVersion=4.2.2"));

        let login = lgn_login_url(
            "25000000",
            "safe-fixture-password",
            "172.30.200.10",
            "2001:db8::10",
        )
        .unwrap();
        assert_eq!(login.scheme(), "https");
        assert_eq!(login.host_str(), Some(LGN_HOST));
        assert_eq!(login.port(), Some(802));
        let pairs: std::collections::HashMap<_, _> = login.query_pairs().into_owned().collect();
        assert_eq!(
            pairs.get("callback").map(String::as_str),
            Some("726427262623")
        );
        assert_eq!(pairs.get("login_method").map(String::as_str), Some("27"));
        assert_eq!(pairs.get("terminal_type").map(String::as_str), Some("25"));
        assert_eq!(pairs.get("login_ip_type").map(String::as_str), Some("26"));
        assert_eq!(pairs.get("encrypt").map(String::as_str), Some("1"));
        assert!(!login.as_str().contains("25000000"));
        assert!(!login.as_str().contains("safe-fixture-password"));
        assert!(!login.as_str().contains("172.30.200.10"));
    }

    #[test]
    fn observed_ip_jsonp_requires_the_expected_address_family() {
        assert_eq!(
            parse_observed_ip(r#"dr1004({"result":1,"ip":"2001:db8::10"});"#, true).unwrap(),
            "2001:db8::10"
        );
        assert!(parse_observed_ip(r#"dr1004({"result":1,"ip":"172.30.0.1"});"#, true).is_err());
        assert!(parse_observed_ip(r#"dr1004({"result":0,"ip":""});"#, true).is_err());
    }

    #[test]
    fn ssid_and_transport_hints_are_specific() {
        assert_eq!(
            login_type_hint("bjut-sushe--5G-bEY5"),
            Some(LoginType::Type1)
        );
        assert_eq!(
            login_type_hint("bjut-suahe-5G-6Y6m"),
            Some(LoginType::Type1)
        );
        assert_eq!(login_type_hint("bjut_wifi"), Some(LoginType::Type2));
        assert_eq!(
            login_type_hint("CU_bjut-sushe-28Au"),
            Some(LoginType::Type1)
        );
        assert_eq!(login_type_hint("room-bjut-sushe-5g"), None);
        assert_eq!(login_type_hint("bjut_wifi_guest"), None);
        assert_eq!(login_type_hint(""), None);
    }

    #[test]
    fn ssid_hints_only_reorder_protocol_probe_candidates() {
        assert_eq!(
            login_probe_candidates("bjut_wifi", "wifi"),
            vec![LoginType::Type2, LoginType::Type1]
        );
        assert_eq!(
            login_probe_candidates("bjut-sushe--5G-bEY5", "wifi"),
            vec![LoginType::Type1, LoginType::Type2]
        );
        assert_eq!(
            login_probe_candidates("CU_bjut-sushe-28Au", "wifi"),
            vec![LoginType::Type1, LoginType::Type2]
        );
        assert_eq!(
            login_probe_candidates("bjut_wifi", "ethernet"),
            vec![LoginType::Type3]
        );
        assert_eq!(
            login_probe_candidates("untrusted-lookalike", "unknown"),
            vec![LoginType::Type1, LoginType::Type2, LoginType::Type3]
        );
    }

    #[test]
    fn type3_portal_detection_is_distinct_from_ipv6_login_readiness() {
        let mut evidence = Type3ProbeEvidence::default();
        evidence.record(
            "https://lgn.bjut.edu.cn:802/eportal/portal/page/loadUserInfo",
            r#"dr1002({"code":1,"user_info":{"account":"25000000"}});"#,
        );
        assert_eq!(evidence.result(), PortalProbeResult::PortalDetected);

        evidence.record(
            "https://lgn6.bjut.edu.cn/drcom/getipv6",
            r#"dr1004({"result":0,"ip":""});"#,
        );
        assert_eq!(evidence.result(), PortalProbeResult::PortalDetected);

        evidence.record(
            "https://lgn6.bjut.edu.cn/drcom/getipv6",
            r#"dr1004({"result":1,"ip":"2001:db8::10"});"#,
        );
        assert_eq!(evidence.result(), PortalProbeResult::LoginReady);
    }

    #[test]
    fn type3_partial_detection_preserves_details_without_being_ready() {
        let detection =
            LoginTypeDetection::from_probe(LoginType::Type3, PortalProbeResult::PortalDetected);
        assert_eq!(detection.login_type, LoginType::Type3);
        assert!(detection.portal_detected);
        assert!(!detection.login_ready);
        assert_eq!(detection.into_ready_login_type(), LoginType::Unknown);
    }

    #[test]
    fn probe_endpoints_keep_http_and_https_ports_distinct() {
        assert_eq!(
            portal_probe_urls(VpnCompatibility::Maximum, &LoginType::Type1),
            vec!["http://10.21.221.98:801/eportal/portal/login"]
        );
        assert_eq!(
            portal_probe_urls(VpnCompatibility::Minimum, &LoginType::Type1),
            vec!["https://10.21.221.98:802/eportal/portal/login"]
        );
        let wired = portal_probe_urls(VpnCompatibility::Maximum, &LoginType::Type3);
        assert_eq!(wired.len(), 2);
        assert!(wired.iter().all(|url| url.contains(":801/")));

        let readiness = login_readiness_probe_urls(VpnCompatibility::Maximum, &LoginType::Type3);
        assert_eq!(readiness.len(), 3);
        assert!(readiness.iter().any(|url| url.contains("/drcom/getipv6")));
    }

    #[test]
    fn probes_require_protocol_specific_response_fingerprints() {
        assert!(probe_body_matches(
            &LoginType::Type1,
            "http://10.21.221.98:801/eportal/portal/login",
            r#"dr1003({"result":0,"msg":"missing parameters"});"#,
        ));
        assert!(probe_body_matches(
            &LoginType::Type2,
            "https://wlgn.bjut.edu.cn/drcom/login",
            "<form><input name=\"DDDDD\"><input name=\"0MKKey\"></form>",
        ));
        assert!(probe_body_matches(
            &LoginType::Type3,
            "http://172.30.201.2:801/eportal/portal/page/loadUserInfo",
            r#"dr1002({"code":1,"user_info":{"account":"25000000"},"user_info_lang":{"account":"账号"}});"#,
        ));
        assert!(!probe_body_matches(
            &LoginType::Type3,
            "http://172.30.201.2:801/eportal/portal/page/loadUserInfo",
            r#"dr1002({"code":1,"user_info_lang":{"account":"账号"}});"#,
        ));
        assert!(!probe_body_matches(
            &LoginType::Type1,
            "http://10.21.221.98:801/eportal/portal/login",
            "generic reverse proxy error page",
        ));
    }
}
