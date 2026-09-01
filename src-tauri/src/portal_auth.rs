use super::{
    query_campus_dns_ipv4, redact_request_error, usable_physical_ipv4, VpnCompatibility, LGN6_HOST,
    LGN_HOST, WLGN_HOST,
};
use crate::network_trust::{campus_wifi_kind, CampusWifiKind};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use reqwest::header::{ACCEPT, CACHE_CONTROL, HOST, REFERER};
use reqwest::{Client, Url};
use serde_json::Value;
use std::fmt::Write as _;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod dorm_tls;

pub(crate) const AMBIGUOUS_LOGIN_RESULT: &str = "认证结果暂无法确认";

/// The physical route that was observed together with the current network
/// identity. Portal probes and credential-bearing requests must share this
/// context so a VPN/TUN route cannot take over between detection and login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PortalRouteContext {
    interface_name: String,
    physical_ipv4: Ipv4Addr,
}

impl PortalRouteContext {
    pub(crate) fn new(interface_name: &str, physical_ipv4: &str) -> Result<Self, String> {
        let interface_name = interface_name.trim();
        if interface_name.is_empty() {
            return Err("无法确定校园网认证所用的物理接口".to_string());
        }
        let physical_ipv4 = usable_physical_ipv4(physical_ipv4).ok_or_else(|| {
            "校园网认证所用的物理接口 IPv4 无效；已拒绝使用 VPN/TUN Fake-IP".to_string()
        })?;
        Ok(Self {
            interface_name: interface_name.to_string(),
            physical_ipv4,
        })
    }

    #[cfg(not(target_os = "android"))]
    pub(crate) fn interface_name(&self) -> &str {
        &self.interface_name
    }

    pub(crate) fn physical_ipv4(&self) -> Ipv4Addr {
        self.physical_ipv4
    }
}

const DORM_HTTP_LOGIN: &str = "http://10.21.221.98:801/eportal/portal/login";
const DORM_HTTP_REFERER: &str = "http://10.21.221.98/";
const DORM_HTTPS_AUTHORITY: &str = "10.21.221.98:802";
const DORM_HTTPS_REFERER: &str = "https://10.21.221.98:802/";
// The dormitory gateway only exposes an IP address to users, while its TLS
// certificate is issued for BJUT hostnames.  Keep this list limited to
// hostnames already used by the captured official portal flows.  The client
// resolves them directly to the dormitory gateway and uses a narrowly scoped
// certificate-expiry pin while retaining chain, name and handshake checks.
// The HTTP Host still matches the captured IP endpoint, and a read-only Type 1
// response must be confirmed before any credential is sent.
const DORM_GATEWAY_IPV4: Ipv4Addr = Ipv4Addr::new(10, 21, 221, 98);
const WIFI_HTTP_LOGIN: &str = "http://10.21.251.3/drcom/login";
const WIFI_HTTPS_LOGIN: &str = "https://wlgn.bjut.edu.cn/drcom/login";
const WIFI_HTTP_LOGOUT: &str = "http://10.21.251.3/drcom/logout";
const WIFI_HTTPS_LOGOUT: &str = "https://wlgn.bjut.edu.cn/drcom/logout";
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
            Self::Type1 => "bjut-sushe",
            Self::Type2 => "bjut_wifi",
            Self::Type3 => "lgn-wired",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) fn display_name(&self) -> &'static str {
        match self {
            Self::Type1 => "bjut-sushe",
            Self::Type2 => "bjut_wifi",
            Self::Type3 => "lgn 有线",
            Self::Unknown => "未识别",
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
        vec![LoginType::Type1, LoginType::Type3]
    } else {
        vec![LoginType::Type1, LoginType::Type2, LoginType::Type3]
    };

    // SSID is only a priority hint. A matching name must never bypass the
    // protocol-specific response probe, because SSIDs can be renamed or
    // spoofed. Ethernet deliberately excludes the Wi-Fi-only Type 2 protocol,
    // but can use both the dormitory Type 1 portal and wired-only Type 3.
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
    route_context: Option<&PortalRouteContext>,
) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .use_rustls_tls();

    if *login_type == LoginType::Type1 && compatibility != VpnCompatibility::Maximum {
        // HTTP/1.1 keeps the explicit Host header from the captured IP-based
        // request separate from the allowlisted TLS SNI alias.
        builder = builder
            .http1_only()
            .use_preconfigured_tls(dorm_tls::client_config()?);
    }

    // Android binds the whole process to the exact ConnectivityManager
    // Network before entering this module. Rebinding a socket by Linux device
    // name here would bypass that Network object's DNS and lifecycle rules.
    // Desktop has no equivalent outer guard, so every portal client must bind
    // at least its local source address. macOS/Linux additionally support an
    // explicit interface binding in reqwest.
    #[cfg(not(target_os = "android"))]
    {
        let route_context = route_context
            .ok_or_else(|| "未取得同一物理接口的网络路由，已停止校园网网关请求".to_string())?;
        builder = builder.local_address(IpAddr::V4(route_context.physical_ipv4()));
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            builder = builder.interface(route_context.interface_name());
        }
        #[cfg(target_os = "windows")]
        let _ = route_context.interface_name();
    }
    #[cfg(target_os = "android")]
    let _ = route_context;
    let hosts: Vec<(&str, Vec<Ipv4Addr>)> = match login_type {
        LoginType::Type1 if compatibility != VpnCompatibility::Maximum => {
            dorm_tls::TLS_HOST_CANDIDATES
                .iter()
                .map(|host| (*host, vec![DORM_GATEWAY_IPV4]))
                .collect()
        }
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
    if *login_type == LoginType::Type1 && compatibility != VpnCompatibility::Maximum
        || matches!(
            compatibility,
            VpnCompatibility::Low | VpnCompatibility::High
        )
    {
        let dns_source = route_context.map(PortalRouteContext::physical_ipv4);
        for (host, fixed_addresses) in hosts {
            let addresses = if compatibility == VpnCompatibility::Low
                && *login_type != LoginType::Type1
            {
                let host_owned = host.to_string();
                tokio::task::spawn_blocking(move || query_campus_dns_ipv4(&host_owned, dns_source))
                    .await
                    .map_err(|error| format!("校园网 DNS 任务失败：{error}"))??
            } else {
                fixed_addresses
            };
            let socket_addresses: Vec<SocketAddr> = addresses
                .into_iter()
                // Explicit URL ports (such as ePortal's 802) take precedence
                // over the resolver entry; zero avoids implying a different
                // service port when the URL has no explicit port.
                .map(|address| SocketAddr::new(IpAddr::V4(address), 0))
                .collect();
            builder = builder.resolve_to_addrs(host, &socket_addresses);
        }
    }
    builder.build().map_err(redact_request_error)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn portal_probe_urls(
    compatibility: VpnCompatibility,
    login_type: &LoginType,
) -> Vec<String> {
    portal_probe_urls_for_route(compatibility, login_type, None)
}

fn portal_probe_urls_for_route(
    compatibility: VpnCompatibility,
    login_type: &LoginType,
    physical_ipv4: Option<Ipv4Addr>,
) -> Vec<String> {
    match login_type {
        // The login endpoint requires a complete query string.  A bare
        // `/eportal/portal/login` request is commonly answered with an empty
        // page (or a redirect), which made a reachable dorm gateway look
        // offline during automatic detection.  loadConfig is read-only and
        // is requested by the real portal page before it submits credentials.
        LoginType::Type1 if compatibility == VpnCompatibility::Maximum => {
            vec![type1_probe_url(DORM_HTTP_LOGIN, physical_ipv4)]
        }
        LoginType::Type1 => dorm_tls::TLS_HOST_CANDIDATES
            .iter()
            .map(|host| type1_probe_url(&type1_https_login_base(host), physical_ipv4))
            .collect(),
        // Dr.COM exposes a read-only status endpoint.  Probing `/login`
        // without the documented JSONP fields is not reliable on all gateway
        // versions, while chkstatus is the same request used by the captured
        // browser flow.
        LoginType::Type2 => vec![type2_probe_url(compatibility)],
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

fn type1_https_login_base(host: &str) -> String {
    format!("https://{host}:802/eportal/portal/login")
}

fn type1_probe_url(login_base: &str, physical_ipv4: Option<Ipv4Addr>) -> String {
    let base = login_base.replace("/portal/login", "/portal/page/loadConfig");
    let mut url = Url::parse(&base).expect("static Type 1 probe URL must be valid");
    url.query_pairs_mut()
        .append_pair("callback", "dr1001")
        .append_pair("program_index", "")
        .append_pair("wlan_vlan_id", "0")
        .append_pair(
            "wlan_user_ip",
            &physical_ipv4
                .map(|address| BASE64.encode(address.to_string()))
                .unwrap_or_default(),
        )
        .append_pair("wlan_user_ipv6", "")
        .append_pair("wlan_user_ssid", "")
        .append_pair("wlan_user_areaid", "")
        .append_pair("wlan_ac_ip", "")
        .append_pair("wlan_ap_mac", "000000000000")
        .append_pair("gw_id", "000000000000")
        .append_pair("jsVersion", "4.X")
        .append_pair("v", &random_request_id())
        .append_pair("lang", "zh");
    url.to_string()
}

fn type2_probe_url(compatibility: VpnCompatibility) -> String {
    let base = if compatibility == VpnCompatibility::Maximum {
        "http://10.21.251.3/drcom/chkstatus"
    } else {
        "https://wlgn.bjut.edu.cn/drcom/chkstatus"
    };
    let mut url = Url::parse(base).expect("static Type 2 probe URL must be valid");
    url.query_pairs_mut()
        .append_pair("callback", "dr1002")
        .append_pair("jsVersion", "4.1")
        .append_pair("v", &random_request_id())
        .append_pair("lang", "zh");
    url.to_string()
}

fn login_readiness_probe_urls(
    compatibility: VpnCompatibility,
    login_type: &LoginType,
    route_context: Option<&PortalRouteContext>,
) -> Vec<String> {
    let mut urls = portal_probe_urls_for_route(
        compatibility,
        login_type,
        route_context.map(PortalRouteContext::physical_ipv4),
    );
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
    pub(crate) timed_out: bool,
}

impl LoginTypeDetection {
    fn not_detected() -> Self {
        Self {
            login_type: LoginType::Unknown,
            portal_detected: false,
            login_ready: false,
            timed_out: false,
        }
    }

    fn from_probe(login_type: LoginType, result: PortalProbeResult) -> Self {
        Self {
            login_type,
            portal_detected: result != PortalProbeResult::NotDetected,
            login_ready: result == PortalProbeResult::LoginReady,
            timed_out: false,
        }
    }

    fn timed_out(login_type: LoginType) -> Self {
        Self {
            login_type,
            portal_detected: false,
            login_ready: false,
            timed_out: true,
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
    route_context: Option<&PortalRouteContext>,
) -> PortalProbeResult {
    // Type 3 maximum compatibility still needs the TLS/SNI-preserving lgn6
    // endpoint to prove that IPv6 login is actually ready.
    let client_compatibility =
        if login_type == LoginType::Type3 && compatibility == VpnCompatibility::Maximum {
            VpnCompatibility::High
        } else {
            compatibility
        };
    let client = portal_client(
        client_compatibility,
        &login_type,
        Duration::from_millis(1800),
        route_context,
    )
    .await;
    let Ok(client) = client else {
        return probe_type1_http_portal_only(compatibility, &login_type, route_context).await;
    };
    let mut type3_evidence = Type3ProbeEvidence::default();
    for url in login_readiness_probe_urls(compatibility, &login_type, route_context) {
        let referer = match login_type {
            LoginType::Type1 if compatibility == VpnCompatibility::Maximum => {
                DORM_HTTP_REFERER.to_string()
            }
            LoginType::Type1 => DORM_HTTPS_REFERER.to_string(),
            LoginType::Type2 if compatibility == VpnCompatibility::Maximum => {
                WIFI_HTTP_REFERER.to_string()
            }
            LoginType::Type2 => WIFI_HTTPS_REFERER.to_string(),
            LoginType::Type3 => LGN_REFERER.to_string(),
            LoginType::Unknown => String::new(),
        };
        let mut request = client
            .get(&url)
            .header(ACCEPT, "*/*")
            .header(REFERER, &referer)
            .header(CACHE_CONTROL, "no-cache, no-store");
        if login_type == LoginType::Type1 && compatibility != VpnCompatibility::Maximum {
            request = request.header(HOST, DORM_HTTPS_AUTHORITY);
        }
        let Ok(response) = request.send().await else {
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
        // The visible bjut-sushe page advertises HTTP:801, but the login API is
        // also available on HTTPS:802 with a certificate issued to BJUT domain
        // names rather than the raw IP address. If none of the allowlisted SNI
        // aliases produced the expected read-only response, HTTP may still
        // prove that the portal exists; credentials remain blocked unless the
        // user explicitly enables temporary Maximum mode.
        probe_type1_http_portal_only(compatibility, &login_type, route_context).await
    }
}

/// Confirms that a Type 1 gateway exists without weakening a configured HTTPS
/// login policy.  This request contains no credentials and is only used after
/// the policy-preserving probe did not become login-ready.
async fn probe_type1_http_portal_only(
    compatibility: VpnCompatibility,
    login_type: &LoginType,
    route_context: Option<&PortalRouteContext>,
) -> PortalProbeResult {
    if *login_type != LoginType::Type1 || compatibility == VpnCompatibility::Maximum {
        return PortalProbeResult::NotDetected;
    }
    let Ok(client) = portal_client(
        VpnCompatibility::Maximum,
        login_type,
        Duration::from_millis(1800),
        route_context,
    )
    .await
    else {
        return PortalProbeResult::NotDetected;
    };
    let url = type1_probe_url(
        DORM_HTTP_LOGIN,
        route_context.map(PortalRouteContext::physical_ipv4),
    );
    let Ok(response) = client
        .get(&url)
        .header(ACCEPT, "*/*")
        .header(REFERER, DORM_HTTP_REFERER)
        .header(CACHE_CONTROL, "no-cache, no-store")
        .send()
        .await
    else {
        return PortalProbeResult::NotDetected;
    };
    if !response.status().is_success() {
        return PortalProbeResult::NotDetected;
    }
    let body = response.text().await.unwrap_or_default();
    if probe_body_matches(login_type, &url, &body) {
        PortalProbeResult::PortalDetected
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
        LoginType::Type1 if url.contains("/page/loadConfig") => {
            jsonp_object(body).is_some_and(|value| {
                let code_ok = value
                    .get("code")
                    .is_some_and(|code| code.as_i64() == Some(1) || code.as_str() == Some("1"));
                code_ok && value.get("data").is_some_and(Value::is_object)
            })
        }
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
    route_context: Option<&PortalRouteContext>,
) -> LoginTypeDetection {
    let mut portal_only = None;
    for candidate in login_probe_candidates(ssid, transport) {
        let result = probe_login_type(compatibility, candidate.clone(), route_context).await;
        if result == PortalProbeResult::LoginReady {
            return LoginTypeDetection::from_probe(candidate, result);
        }
        if result == PortalProbeResult::PortalDetected && portal_only.is_none() {
            portal_only = Some(LoginTypeDetection::from_probe(candidate, result));
        }
    }
    portal_only.unwrap_or_else(LoginTypeDetection::not_detected)
}

pub(crate) async fn diagnose_login_gateways(
    compatibility: VpnCompatibility,
    ssid: &str,
    transport: &str,
    route_context: Option<&PortalRouteContext>,
) -> Vec<LoginTypeDetection> {
    const DIAGNOSTIC_CANDIDATE_BUDGET: Duration = Duration::from_millis(2200);
    let mut candidates = login_probe_candidates(ssid, transport);
    // Diagnostics are observational and never send credentials. Probe all
    // three documented gateways even when Windows reports an unknown or
    // Ethernet transport: wired adapters, USB docks and campus bridge devices
    // can expose bjut_wifi while still lacking a WLAN identity. Automatic
    // login keeps the stricter transport-specific candidate set above.
    for candidate in [LoginType::Type1, LoginType::Type2, LoginType::Type3] {
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    let probes = candidates.into_iter().map(|candidate| async move {
        match tokio::time::timeout(
            DIAGNOSTIC_CANDIDATE_BUDGET,
            probe_login_type(compatibility, candidate.clone(), route_context),
        )
        .await
        {
            Ok(result) => LoginTypeDetection::from_probe(candidate, result),
            Err(_) => LoginTypeDetection::timed_out(candidate),
        }
    });
    // Diagnostics must still inspect every applicable gateway, but independent
    // bjut-sushe/bjut_wifi/lgn read-only probes do not need to wait for one
    // another. The bounded concurrent probe reduces a non-campus Wi-Fi check
    // from roughly three sequential timeouts to one diagnostic budget.
    futures_util::future::join_all(probes).await
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

fn lgn_logout_url(local_ipv4: &str, observed_ipv6: &str) -> Result<Url, String> {
    let mut url = Url::parse("https://lgn.bjut.edu.cn:802/eportal/portal/logout")
        .map_err(|error| error.to_string())?;
    let fields = [
        ("callback", "dr1008"),
        ("login_method", "1"),
        ("user_account", "drcom"),
        ("user_password", "123"),
        ("ac_logout", "0"),
        ("register_mode", "1"),
        ("wlan_user_ip", local_ipv4),
        ("wlan_user_ipv6", observed_ipv6),
        ("wlan_vlan_id", "0"),
        ("wlan_user_mac", "000000000000"),
        ("wlan_ac_ip", ""),
        ("wlan_ac_name", ""),
        ("jsVersion", LGN_JS_VERSION),
        ("program_index", LGN_PROGRAM_INDEX),
        ("page_index", LGN_PAGE_INDEX),
    ];
    {
        let mut query = url.query_pairs_mut();
        for (name, value) in fields {
            query.append_pair(name, &eportal_encrypt(value));
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
    route_context: Option<&PortalRouteContext>,
    destination: &str,
    network_label: &str,
) -> Result<String, String> {
    if let Some(route_context) = route_context {
        return Ok(route_context.physical_ipv4().to_string());
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = destination;
        Err(format!(
            "{network_label}登录前未取得同一物理接口的路由；已阻止凭据经过 VPN/TUN"
        ))
    }

    #[cfg(target_os = "android")]
    {
        let routed = super::route_source_ipv4(destination);
        usable_physical_ipv4(&routed)
            .map(|address| address.to_string())
            .ok_or_else(|| {
                format!(
                    "{network_label}登录前无法确定物理接口 IPv4；请检查 Wi-Fi/有线接口或 VPN 分流"
                )
            })
    }
}

async fn login_lgn_once(
    client: &Client,
    user: &str,
    pass: &str,
    route_context: Option<&PortalRouteContext>,
) -> Result<(bool, String), String> {
    let local_ipv4 = login_source_ipv4(route_context, "172.30.201.2:802", "lgn 有线")?;
    let observed_ipv6 = fetch_lgn_observed_ipv6(client).await?;
    let login_url = lgn_login_url(user, pass, &local_ipv4, &observed_ipv6)?;
    get_jsonp_login(client, login_url, LGN_REFERER, "lgn 有线登录", None, None).await
}

async fn fetch_lgn_observed_ipv6(client: &Client) -> Result<String, String> {
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
    parse_observed_ip(&ipv6_body, true)
}

fn type1_login_url(
    login_base: &str,
    user: &str,
    pass: &str,
    local_ip: &str,
) -> Result<Url, String> {
    let mut url = Url::parse(login_base).map_err(|error| error.to_string())?;
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
        // The successful HTTPS:802 capture sends terminal_type=1. Earlier
        // examples containing 3 were page-specific and did not match the
        // observed dormitory login request.
        .append_pair("terminal_type", "1")
        .append_pair("lang", "zh-cn")
        .append_pair("v", &random_request_id())
        .append_pair("lang", "zh");
    Ok(url)
}

fn type1_logout_url(
    login_base: &str,
    user: &str,
    pass: &str,
    local_ip: &str,
) -> Result<Url, String> {
    let logout_base = login_base.replace("/portal/login", "/portal/logout");
    let mut url = Url::parse(&logout_base).map_err(|error| error.to_string())?;
    let account = if user.to_ascii_lowercase().ends_with("@campus") {
        user.to_string()
    } else {
        format!("{user}@campus")
    };
    url.query_pairs_mut()
        .append_pair("callback", "dr1004")
        .append_pair("login_method", "1")
        .append_pair("user_account", &account)
        .append_pair("user_password", pass)
        .append_pair("ac_logout", "0")
        .append_pair("register_mode", "0")
        .append_pair("wlan_user_ip", local_ip)
        .append_pair("wlan_user_ipv6", "")
        .append_pair("wlan_vlan_id", "0")
        .append_pair("wlan_user_mac", "000000000000")
        .append_pair("wlan_ac_ip", "")
        .append_pair("wlan_ac_name", "")
        .append_pair("jsVersion", "4.2.1")
        .append_pair("v", &random_request_id())
        .append_pair("lang", "zh");
    Ok(url)
}

fn type2_logout_url(compatibility: VpnCompatibility) -> Result<Url, String> {
    let base = if compatibility == VpnCompatibility::Maximum {
        WIFI_HTTP_LOGOUT
    } else {
        WIFI_HTTPS_LOGOUT
    };
    let mut url = Url::parse(base).map_err(|error| error.to_string())?;
    url.query_pairs_mut()
        .append_pair("callback", "dr1004")
        .append_pair("jsVersion", "4.1")
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
    host_override: Option<&str>,
) -> Result<(bool, String), String> {
    let mut request = client
        .get(url)
        .header(ACCEPT, "*/*")
        .header(REFERER, referer)
        .header(CACHE_CONTROL, "no-cache, no-store");
    if let Some(host) = host_override {
        request = request.header(HOST, host);
    }
    let response = request
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

async fn select_type1_tls_host(
    client: &Client,
    route_context: Option<&PortalRouteContext>,
) -> Result<&'static str, String> {
    let physical_ipv4 = route_context.map(PortalRouteContext::physical_ipv4);
    for host in dorm_tls::TLS_HOST_CANDIDATES {
        let login_base = type1_https_login_base(host);
        let probe_url = type1_probe_url(&login_base, physical_ipv4);
        let referer = DORM_HTTPS_REFERER;
        let Ok(response) = client
            .get(&probe_url)
            .header(ACCEPT, "*/*")
            .header(REFERER, referer)
            .header(HOST, DORM_HTTPS_AUTHORITY)
            .header(CACHE_CONTROL, "no-cache, no-store")
            .send()
            .await
        else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let body = response.text().await.unwrap_or_default();
        if probe_body_matches(&LoginType::Type1, &probe_url, &body) {
            return Ok(host);
        }
    }
    Err(
        "已定位 bjut-sushe 网关，但 HTTPS:802 返回的证书或协议响应未能通过校验；应用未发送账号密码"
            .to_string(),
    )
}

pub(crate) async fn login_to_campus_network_rust(
    login_type: LoginType,
    user: &str,
    pass: &str,
    compatibility: VpnCompatibility,
    route_context: Option<&PortalRouteContext>,
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
    let client = portal_client(
        client_compatibility,
        &login_type,
        Duration::from_secs(5),
        route_context,
    )
    .await?;
    match login_type {
        LoginType::Type1 => {
            let destination = if compatibility == VpnCompatibility::Maximum {
                "10.21.221.98:801"
            } else {
                "10.21.221.98:802"
            };
            let local_ip = login_source_ipv4(route_context, destination, "bjut-sushe")?;
            let (login_base, referer, hint, host_override) =
                if compatibility == VpnCompatibility::Maximum {
                    (
                        DORM_HTTP_LOGIN.to_string(),
                        DORM_HTTP_REFERER.to_string(),
                        None,
                        None,
                    )
                } else {
                    // Reconfirm the TLS/SNI alias immediately before constructing
                    // the credential-bearing URL. A successful generic gateway
                    // probe is not enough to authorize credentials for a
                    // different hostname or response protocol.
                    let host = select_type1_tls_host(&client, route_context).await?;
                    let login_base = type1_https_login_base(host);
                    (
                        login_base,
                        DORM_HTTPS_REFERER.to_string(),
                        None,
                        Some(DORM_HTTPS_AUTHORITY),
                    )
                };
            get_jsonp_login(
                &client,
                type1_login_url(&login_base, user, pass, &local_ip)?,
                &referer,
                "bjut-sushe 登录",
                hint,
                host_override,
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
                None,
            )
            .await
        }
        LoginType::Type3 => login_lgn_once(&client, user, pass, route_context).await,
        LoginType::Unknown => Err("未设定的登录类型".to_string()),
    }
}

pub(crate) async fn logout_from_campus_network_rust(
    logout_method: LoginType,
    user: Option<&str>,
    pass: Option<&str>,
    compatibility: VpnCompatibility,
    route_context: Option<&PortalRouteContext>,
) -> Result<(bool, String), String> {
    let client_compatibility =
        if logout_method == LoginType::Type3 && compatibility == VpnCompatibility::Maximum {
            VpnCompatibility::High
        } else {
            compatibility
        };
    let client = portal_client(
        client_compatibility,
        &logout_method,
        Duration::from_secs(5),
        route_context,
    )
    .await?;
    match logout_method {
        LoginType::Type1 => {
            let user = user
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "bjut-sushe 注销需要当前账号的已保存凭据".to_string())?;
            let pass = pass
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "bjut-sushe 注销需要当前账号的已保存凭据".to_string())?;
            let destination = if compatibility == VpnCompatibility::Maximum {
                "10.21.221.98:801"
            } else {
                "10.21.221.98:802"
            };
            let local_ip = login_source_ipv4(route_context, destination, "bjut-sushe")?;
            let (login_base, referer, host_override) = if compatibility == VpnCompatibility::Maximum
            {
                (DORM_HTTP_LOGIN.to_string(), DORM_HTTP_REFERER, None)
            } else {
                let host = select_type1_tls_host(&client, route_context).await?;
                (
                    type1_https_login_base(host),
                    DORM_HTTPS_REFERER,
                    Some(DORM_HTTPS_AUTHORITY),
                )
            };
            get_jsonp_login(
                &client,
                type1_logout_url(&login_base, user, pass, &local_ip)?,
                referer,
                "bjut-sushe 注销",
                None,
                host_override,
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
                type2_logout_url(compatibility)?,
                referer,
                "bjut_wifi 注销",
                None,
                None,
            )
            .await
        }
        LoginType::Type3 => {
            let local_ipv4 = login_source_ipv4(route_context, "172.30.201.2:802", "lgn")?;
            let observed_ipv6 = fetch_lgn_observed_ipv6(&client).await?;
            get_jsonp_login(
                &client,
                lgn_logout_url(&local_ipv4, &observed_ipv6)?,
                LGN_REFERER,
                "lgn 注销",
                None,
                None,
            )
            .await
        }
        LoginType::Unknown => Err("未识别当前校园网注销类型".to_string()),
    }
}

pub(crate) fn login_result_is_ambiguous(error: &str) -> bool {
    error.starts_with(AMBIGUOUS_LOGIN_RESULT)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PortalLoginFailureDisposition {
    /// The gateway explicitly rejected this username/password. Trying the next
    /// configured account is meaningful.
    CredentialRejected,
    /// The physical client IP already owns a portal session. This is not an
    /// account-password failure and must never cause every credential to be
    /// submitted in sequence.
    SessionAlreadyOnline,
    /// The response is environmental, rate-limited, or otherwise not specific
    /// to one credential. Stop the account traversal conservatively.
    StopTraversal,
}

pub(crate) fn classify_portal_login_failure(message: &str) -> PortalLoginFailureDisposition {
    let normalized = message.trim().to_ascii_lowercase();
    if [
        "已经在线",
        "已在线",
        "already online",
        "has been online",
        "ip_already_online",
        "same ip online",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
    {
        return PortalLoginFailureDisposition::SessionAlreadyOnline;
    }
    if [
        "ldap auth error",
        "userid error",
        "账号不存在",
        "用户不存在",
        "密码错误",
        "余额不足",
        "欠费",
        "账号停机",
        "账户停机",
        "已停机",
        "invalid password",
        "wrong password",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
    {
        return PortalLoginFailureDisposition::CredentialRejected;
    }
    PortalLoginFailureDisposition::StopTraversal
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
    fn portal_failures_only_traverse_accounts_for_explicit_credential_rejections() {
        assert_eq!(
            classify_portal_login_failure("IP: 10.126.80.236 已经在线！"),
            PortalLoginFailureDisposition::SessionAlreadyOnline
        );
        assert_eq!(
            classify_portal_login_failure("Rad:ldap auth error"),
            PortalLoginFailureDisposition::CredentialRejected
        );
        assert_eq!(
            classify_portal_login_failure("Rad:userid error1"),
            PortalLoginFailureDisposition::CredentialRejected
        );
        assert_eq!(
            classify_portal_login_failure("余额不足，账号已停机"),
            PortalLoginFailureDisposition::CredentialRejected
        );
        assert_eq!(
            classify_portal_login_failure("认证服务器繁忙，请稍后再试"),
            PortalLoginFailureDisposition::StopTraversal
        );
    }

    #[test]
    fn login_urls_use_the_documented_ports_and_fields() {
        let dorm = type1_login_url(DORM_HTTP_LOGIN, "25000000", "p!", "10.126.21.113").unwrap();
        assert_eq!(dorm.port(), Some(801));
        assert_eq!(dorm.scheme(), "http");
        assert!(dorm.as_str().contains("terminal_type=1"));
        assert!(dorm.as_str().contains("wlan_user_ip=10.126.21.113"));
        assert!(dorm.as_str().contains("user_account=25000000%40campus"));
        assert_eq!(
            dorm.query_pairs()
                .filter(|(name, _)| name == "lang")
                .count(),
            2
        );

        let dorm_https = type1_login_url(
            &type1_https_login_base(WLGN_HOST),
            "25000000",
            "p!",
            "10.126.21.113",
        )
        .unwrap();
        assert_eq!(dorm_https.scheme(), "https");
        assert_eq!(dorm_https.host_str(), Some(WLGN_HOST));
        assert_eq!(dorm_https.port(), Some(802));

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
    fn logout_urls_match_the_captured_portal_shapes() {
        let dorm = type1_logout_url(
            DORM_HTTP_LOGIN,
            "25000000",
            "test-password",
            "10.126.21.113",
        )
        .unwrap();
        assert_eq!(dorm.path(), "/eportal/portal/logout");
        assert_eq!(dorm.port(), Some(801));
        assert!(dorm
            .query_pairs()
            .any(|(name, value)| { name == "user_account" && value == "25000000@campus" }));
        assert!(dorm
            .query_pairs()
            .any(|(name, value)| name == "register_mode" && value == "0"));

        let wifi = type2_logout_url(VpnCompatibility::High).unwrap();
        assert_eq!(wifi.as_str().split('?').next().unwrap(), WIFI_HTTPS_LOGOUT);
        assert!(wifi
            .query_pairs()
            .any(|(name, value)| name == "callback" && value == "dr1004"));

        let lgn = lgn_logout_url("172.26.33.104", "2001:db8::1").unwrap();
        assert_eq!(lgn.path(), "/eportal/portal/logout");
        assert!(lgn
            .query_pairs()
            .any(|(name, value)| { name == "user_account" && value == eportal_encrypt("drcom") }));
        assert!(lgn
            .query_pairs()
            .any(|(name, value)| { name == "user_password" && value == eportal_encrypt("123") }));
        assert!(lgn
            .query_pairs()
            .any(|(name, value)| name == "encrypt" && value == "1"));
    }

    #[test]
    fn login_source_uses_the_physical_interface_and_rejects_tun_fake_ip() {
        let route = PortalRouteContext::new("en0", "10.3.219.173").unwrap();
        assert_eq!(
            login_source_ipv4(Some(&route), "10.21.221.98:801", "fixture").unwrap(),
            "10.3.219.173"
        );
        assert!(PortalRouteContext::new("en0", "198.18.12.34")
            .unwrap_err()
            .contains("Fake-IP"));
        assert!(PortalRouteContext::new("", "10.3.219.173")
            .unwrap_err()
            .contains("物理接口"));
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
            vec![LoginType::Type1, LoginType::Type3]
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
    }

    #[test]
    fn probe_endpoints_keep_http_and_https_ports_distinct() {
        let dorm_http = portal_probe_urls(VpnCompatibility::Maximum, &LoginType::Type1);
        assert_eq!(dorm_http.len(), 1);
        assert!(dorm_http[0].starts_with("http://10.21.221.98:801/eportal/portal/page/loadConfig?"));
        let dorm_https = portal_probe_urls(VpnCompatibility::Minimum, &LoginType::Type1);
        assert_eq!(dorm_https.len(), dorm_tls::TLS_HOST_CANDIDATES.len());
        assert!(dorm_https.iter().all(|url| {
            let parsed = Url::parse(url).unwrap();
            parsed.scheme() == "https"
                && parsed.port() == Some(802)
                && dorm_tls::TLS_HOST_CANDIDATES.contains(&parsed.host_str().unwrap_or_default())
                && parsed.path() == "/eportal/portal/page/loadConfig"
        }));
        let dorm_with_ip = portal_probe_urls_for_route(
            VpnCompatibility::Maximum,
            &LoginType::Type1,
            Some(Ipv4Addr::new(10, 126, 21, 113)),
        );
        let dorm_with_ip_url = Url::parse(&dorm_with_ip[0]).unwrap();
        assert_eq!(
            dorm_with_ip_url
                .query_pairs()
                .find(|(name, _)| name == "wlan_user_ip")
                .map(|(_, value)| value.into_owned()),
            Some("MTAuMTI2LjIxLjExMw==".to_string())
        );
        let wifi_https = portal_probe_urls(VpnCompatibility::High, &LoginType::Type2);
        assert_eq!(wifi_https.len(), 1);
        assert!(wifi_https[0].starts_with("https://wlgn.bjut.edu.cn/drcom/chkstatus?"));
        let wifi_http = portal_probe_urls(VpnCompatibility::Maximum, &LoginType::Type2);
        assert_eq!(wifi_http.len(), 1);
        assert!(wifi_http[0].starts_with("http://10.21.251.3/drcom/chkstatus?"));
        let wired = portal_probe_urls(VpnCompatibility::Maximum, &LoginType::Type3);
        assert_eq!(wired.len(), 2);
        assert!(wired.iter().all(|url| url.contains(":801/")));

        let readiness =
            login_readiness_probe_urls(VpnCompatibility::Maximum, &LoginType::Type3, None);
        assert_eq!(readiness.len(), 3);
        assert!(readiness.iter().any(|url| url.contains("/drcom/getipv6")));
    }

    #[test]
    fn probes_require_protocol_specific_response_fingerprints() {
        assert!(probe_body_matches(
            &LoginType::Type1,
            "http://10.21.221.98:801/eportal/portal/page/loadConfig",
            r#"dr1001({"code":1,"data":{"program_index":"demo"}});"#,
        ));
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
