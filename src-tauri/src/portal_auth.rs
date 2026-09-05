use super::{
    query_campus_dns_ipv4, redact_request_error, usable_physical_ipv4, VpnCompatibility, LGN6_HOST,
    LGN_HOST, WLGN_HOST,
};
use crate::network_probe::NETWORK_PROBE_TIMEOUT;
use crate::network_trust::{campus_wifi_kind, CampusWifiKind};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures_util::{
    future::BoxFuture,
    stream::{FuturesOrdered, FuturesUnordered},
    FutureExt, StreamExt,
};
use reqwest::header::{ACCEPT, CACHE_CONTROL, HOST, REFERER};
use reqwest::{Client, ClientBuilder, Url};
use serde_json::Value;
use std::fmt::Write as _;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod dorm_tls;
#[cfg(target_os = "windows")]
mod lgn_ipv6_windows;

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
const LGN6_GATEWAY_IPV6: [Ipv6Addr; 2] = [
    Ipv6Addr::new(0x2001, 0x0da8, 0x0216, 0x30c9, 0, 0, 0, 0x0002),
    Ipv6Addr::new(0x2001, 0x0da8, 0x0216, 0x30c9, 0, 0, 0, 0x000a),
];
const WIFI_HTTP_LOGIN: &str = "http://10.21.251.3/drcom/login";
const WIFI_HTTPS_LOGIN: &str = "https://wlgn.bjut.edu.cn/drcom/login";
const WIFI_HTTP_LOGOUT: &str = "http://10.21.251.3/drcom/logout";
const WIFI_HTTPS_LOGOUT: &str = "https://wlgn.bjut.edu.cn/drcom/logout";
const WIFI_HTTP_REFERER: &str = "http://10.21.251.3/";
const WIFI_HTTPS_REFERER: &str = "https://wlgn.bjut.edu.cn/";
const LGN_REFERER: &str = "https://lgn.bjut.edu.cn/";
const LGN_ROOT: &str = "https://lgn.bjut.edu.cn/";
const LGN6_ROOT: &str = "https://lgn6.bjut.edu.cn/";
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

fn lgn_wired_route_hint(route_context: Option<&PortalRouteContext>) -> bool {
    route_context.is_some_and(|context| {
        let octets = context.physical_ipv4().octets();
        // The supplied lgn wired capture uses 172.26.33.0/24, together with
        // BJUT IPv6/DNS. Keep the automatic hint at the enclosing 172.26/16
        // campus client range; an actual lgn response is still mandatory.
        octets[0] == 172 && octets[1] == 26
    })
}

fn login_probe_candidates(
    ssid: &str,
    transport: &str,
    route_context: Option<&PortalRouteContext>,
) -> Vec<LoginType> {
    let lgn_wired_hint =
        transport.eq_ignore_ascii_case("ethernet") && lgn_wired_route_hint(route_context);
    let mut candidates = if transport.eq_ignore_ascii_case("wifi") {
        vec![LoginType::Type1, LoginType::Type2]
    } else if lgn_wired_hint {
        vec![LoginType::Type3, LoginType::Type1]
    } else if transport.eq_ignore_ascii_case("ethernet") {
        vec![LoginType::Type1, LoginType::Type3]
    } else if lgn_wired_route_hint(route_context) {
        vec![LoginType::Type3, LoginType::Type1, LoginType::Type2]
    } else {
        vec![LoginType::Type1, LoginType::Type2, LoginType::Type3]
    };

    // SSID is only a priority hint. A matching name must never bypass the
    // protocol-specific response probe, because SSIDs can be renamed or
    // spoofed. Ethernet deliberately excludes the Wi-Fi-only Type 2 protocol,
    // but can use both the dormitory Type 1 portal and wired-only Type 3.
    if !lgn_wired_hint {
        if let Some(hint) = login_type_hint(ssid) {
            if let Some(position) = candidates.iter().position(|candidate| *candidate == hint) {
                candidates.swap(0, position);
            }
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
    let hosts: Vec<(&str, Vec<IpAddr>)> = match login_type {
        LoginType::Type1 if compatibility != VpnCompatibility::Maximum => {
            dorm_tls::TLS_HOST_CANDIDATES
                .iter()
                .map(|host| (*host, vec![IpAddr::V4(DORM_GATEWAY_IPV4)]))
                .collect()
        }
        LoginType::Type2 => vec![(WLGN_HOST, vec![IpAddr::V4(Ipv4Addr::new(10, 21, 251, 3))])],
        LoginType::Type3 => vec![
            (
                LGN_HOST,
                vec![
                    IpAddr::V4(Ipv4Addr::new(172, 30, 201, 2)),
                    IpAddr::V4(Ipv4Addr::new(172, 30, 201, 10)),
                ],
            ),
            (
                LGN6_HOST,
                LGN6_GATEWAY_IPV6.into_iter().map(IpAddr::V6).collect(),
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
                && host != LGN6_HOST
            {
                query_campus_dns_ipv4(host, dns_source)
                    .await?
                    .into_iter()
                    .map(IpAddr::V4)
                    .collect()
            } else {
                fixed_addresses
            };
            let socket_addresses: Vec<SocketAddr> = addresses
                .into_iter()
                // Explicit URL ports (such as ePortal's 802) take precedence
                // over the resolver entry; zero avoids implying a different
                // service port when the URL has no explicit port.
                .map(|address| SocketAddr::new(address, 0))
                .collect();
            builder = builder.resolve_to_addrs(host, &socket_addresses);
        }
    }
    builder.build().map_err(redact_request_error)
}

fn bind_lgn_ipv6_route(
    builder: ClientBuilder,
    route_context: Option<&PortalRouteContext>,
) -> Result<ClientBuilder, String> {
    // reqwest/hyper filters out every IPv6 destination when local_address is
    // IPv4. Discovery therefore needs its own IPv6-only connector; the
    // credential-bearing ePortal connector remains bound to physical_ipv4.
    let builder = builder.local_address(IpAddr::V6(Ipv6Addr::UNSPECIFIED));
    #[cfg(not(target_os = "android"))]
    let route_context = route_context
        .ok_or_else(|| "未取得同一物理接口的网络路由，已停止 lgn IPv6 地址发现".to_string())?;
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
    let builder = builder.interface(route_context.interface_name());
    #[cfg(target_os = "windows")]
    let builder = builder.local_address(IpAddr::V6(lgn_ipv6_windows::source_ipv6(
        route_context.physical_ipv4(),
    )?));
    // Android has already bound the process to the selected Network. An
    // unspecified IPv6 source preserves that binding without a device override.
    #[cfg(target_os = "android")]
    let _ = route_context;
    Ok(builder)
}

/// The LGN6 address-discovery endpoint uses an older TLS deployment that is
/// accepted by the platform/OpenSSL verifier used by browsers and libcurl but
/// can fail during a rustls handshake. Retain certificate/hostname checks and
/// HTTPS/SNI while resolving LGN6 to the captured IPv6 gateways.
fn lgn_ipv6_client(
    compatibility: VpnCompatibility,
    timeout: Duration,
    route_context: Option<&PortalRouteContext>,
) -> Result<Client, String> {
    let builder = Client::builder()
        .timeout(timeout)
        .connect_timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .use_native_tls();
    let mut builder = bind_lgn_ipv6_route(builder, route_context)?;

    if compatibility != VpnCompatibility::Minimum {
        let lgn6_addresses = LGN6_GATEWAY_IPV6.map(|address| SocketAddr::new(address.into(), 0));
        builder = builder.resolve_to_addrs(LGN6_HOST, &lgn6_addresses);
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
    let urls = portal_probe_urls_for_route(
        compatibility,
        login_type,
        route_context.map(PortalRouteContext::physical_ipv4),
    );
    if *login_type != LoginType::Type3 {
        return urls;
    }

    // The TLS-protected landing pages provide a pre-authentication fingerprint,
    // unlike loadUserInfo, which may omit user_info until a session exists.
    // Put them first so diagnostics do not spend their entire budget waiting
    // for a session-dependent endpoint.
    let mut readiness = vec![LGN_ROOT.to_string(), LGN6_ROOT.to_string()];
    readiness.extend(
        urls.into_iter()
            .filter(|url| url != LGN_ROOT && url != LGN6_ROOT),
    );
    if !readiness.iter().any(|url| url.contains("/drcom/getipv6")) {
        if let Ok(ipv6_url) = lgn_observed_ipv6_url() {
            readiness.push(ipv6_url.to_string());
        }
    }
    readiness
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
            // portal is present. A usable address enables dual-stack login;
            // otherwise the same verified portal can use the IPv4 fallback.
            self.portal_detected |=
                jsonp_object(body).is_some_and(|value| value.get("result").is_some());
            self.ipv6_login_ready |= parse_observed_ip(body, true).is_ok();
            return;
        }
        self.portal_detected |= probe_body_matches(&LoginType::Type3, url, body);
    }

    fn result(&self) -> PortalProbeResult {
        // lgn supports a documented single-IPv4 fallback: when IPv6 discovery
        // is unavailable, the encrypted login request leaves
        // wlan_user_ipv6 empty. A structurally verified lgn portal is
        // therefore login-ready even without an observed IPv6 address.
        if self.ipv6_login_ready || self.portal_detected {
            PortalProbeResult::LoginReady
        } else {
            PortalProbeResult::NotDetected
        }
    }
}

async fn finish_portal_probes(
    login_type: LoginType,
    probes: Vec<BoxFuture<'_, PortalProbeResult>>,
    budget: Duration,
) -> LoginTypeDetection {
    let mut pending: FuturesUnordered<_> = probes.into_iter().collect();
    let deadline = tokio::time::Instant::now() + budget;
    let mut evidence = PortalProbeResult::NotDetected;
    loop {
        match tokio::time::timeout_at(deadline, pending.next()).await {
            Ok(Some(PortalProbeResult::LoginReady)) => {
                return LoginTypeDetection::from_probe(login_type, PortalProbeResult::LoginReady);
            }
            Ok(Some(PortalProbeResult::PortalDetected)) => {
                evidence = PortalProbeResult::PortalDetected
            }
            Ok(Some(PortalProbeResult::NotDetected)) => {}
            Ok(None) => return LoginTypeDetection::from_probe(login_type, evidence),
            Err(_) => {
                let mut result = LoginTypeDetection::from_probe(login_type, evidence);
                result.timed_out = true;
                return result;
            }
        }
    }
}

async fn probe_login_type(
    compatibility: VpnCompatibility,
    login_type: LoginType,
    route_context: Option<&PortalRouteContext>,
) -> LoginTypeDetection {
    let mut probes = vec![probe_primary_portal(compatibility, &login_type, route_context).boxed()];
    if login_type == LoginType::Type3 {
        // IPv4 campus DNS failure must not prevent independent IPv6 evidence
        // from being observed. Neither branch waits for the other's client.
        probes.push(
            async {
                let Ok(client) =
                    lgn_ipv6_client(compatibility, NETWORK_PROBE_TIMEOUT, route_context)
                else {
                    return PortalProbeResult::NotDetected;
                };
                let mut urls = vec![LGN6_ROOT.to_string()];
                if let Ok(url) = lgn_observed_ipv6_url() {
                    urls.insert(0, url.to_string());
                }
                probe_portal_urls(&client, &login_type, urls, LGN_REFERER, None).await
            }
            .boxed(),
        );
    } else if login_type == LoginType::Type1 && compatibility != VpnCompatibility::Maximum {
        // Retain read-only HTTP evidence if TLS is unavailable. A fast HTTP
        // response cannot outrank a verified TLS login endpoint.
        probes
            .push(probe_type1_http_portal_only(compatibility, &login_type, route_context).boxed());
    }
    finish_portal_probes(login_type.clone(), probes, NETWORK_PROBE_TIMEOUT).await
}

async fn probe_primary_portal(
    compatibility: VpnCompatibility,
    login_type: &LoginType,
    route_context: Option<&PortalRouteContext>,
) -> PortalProbeResult {
    let client_compatibility =
        if *login_type == LoginType::Type3 && compatibility == VpnCompatibility::Maximum {
            VpnCompatibility::High
        } else {
            compatibility
        };
    let Ok(client) = portal_client(
        client_compatibility,
        login_type,
        NETWORK_PROBE_TIMEOUT,
        route_context,
    )
    .await
    else {
        return PortalProbeResult::NotDetected;
    };
    let referer = match login_type {
        LoginType::Type1 if compatibility == VpnCompatibility::Maximum => DORM_HTTP_REFERER,
        LoginType::Type1 => DORM_HTTPS_REFERER,
        LoginType::Type2 if compatibility == VpnCompatibility::Maximum => WIFI_HTTP_REFERER,
        LoginType::Type2 => WIFI_HTTPS_REFERER,
        LoginType::Type3 => LGN_REFERER,
        LoginType::Unknown => "",
    };
    let urls = login_readiness_probe_urls(compatibility, login_type, route_context)
        .into_iter()
        .filter(|url| !Url::parse(url).is_ok_and(|url| url.host_str() == Some(LGN6_HOST)))
        .collect();
    let host_override = (*login_type == LoginType::Type1
        && compatibility != VpnCompatibility::Maximum)
        .then_some(DORM_HTTPS_AUTHORITY);
    probe_portal_urls(&client, login_type, urls, referer, host_override).await
}

async fn probe_portal_urls(
    client: &Client,
    login_type: &LoginType,
    urls: Vec<String>,
    referer: &str,
    host_override: Option<&str>,
) -> PortalProbeResult {
    let mut probes: FuturesUnordered<_> = urls
        .into_iter()
        .map(|url| async move {
            let mut request = client
                .get(&url)
                .header(ACCEPT, "*/*")
                .header(REFERER, referer)
                .header(CACHE_CONTROL, "no-cache, no-store");
            if let Some(host) = host_override {
                request = request.header(HOST, host);
            }
            let Ok(response) = request.send().await else {
                return PortalProbeResult::NotDetected;
            };
            if !response.status().is_success() {
                return PortalProbeResult::NotDetected;
            }
            let body = response.text().await.unwrap_or_default();
            if *login_type == LoginType::Type3 {
                let mut evidence = Type3ProbeEvidence::default();
                evidence.record(&url, &body);
                evidence.result()
            } else if probe_body_matches(login_type, &url, &body) {
                PortalProbeResult::LoginReady
            } else {
                PortalProbeResult::NotDetected
            }
        })
        .collect();
    while let Some(result) = probes.next().await {
        if result == PortalProbeResult::LoginReady {
            return result;
        }
    }
    PortalProbeResult::NotDetected
}

/// Confirms that a Type 1 gateway exists without weakening a configured HTTPS
/// login policy. This request contains no credentials; its evidence is only
/// selected if the policy-preserving probes do not become login-ready.
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
        NETWORK_PROBE_TIMEOUT,
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
            (normalized.contains("dr.comwebloginid_")
                && normalized.contains("authuserfield")
                && normalized.contains("ddddd"))
                || normalized.contains("name=\"v6ip\"")
                || normalized.contains("name='v6ip'")
                || normalized.contains("lgn.bjut.edu.cn")
        }
        LoginType::Unknown => false,
    }
}

async fn select_prioritized_probes<F: std::future::Future<Output = LoginTypeDetection>>(
    probes: impl IntoIterator<Item = F>,
) -> LoginTypeDetection {
    // Poll concurrently, yield in physical-link priority order. As soon as a
    // preferred gateway is verified, discard lower-priority pending probes.
    let mut pending: FuturesOrdered<_> = probes.into_iter().collect();
    while let Some(result) = pending.next().await {
        if result.portal_detected {
            return result;
        }
    }
    LoginTypeDetection::not_detected()
}

pub(crate) async fn detect_login_type_details_rust(
    compatibility: VpnCompatibility,
    ssid: &str,
    transport: &str,
    route_context: Option<&PortalRouteContext>,
) -> LoginTypeDetection {
    let probes = login_probe_candidates(ssid, transport, route_context)
        .into_iter()
        .map(|candidate| probe_login_type(compatibility, candidate, route_context));
    select_prioritized_probes(probes).await
}

pub(crate) async fn diagnose_login_gateways(
    compatibility: VpnCompatibility,
    ssid: &str,
    transport: &str,
    route_context: Option<&PortalRouteContext>,
) -> Vec<LoginTypeDetection> {
    let mut candidates = login_probe_candidates(ssid, transport, route_context);
    // Diagnostics inspect all three gateways; authentication keeps the stricter
    // transport-specific candidate set. All share one three-second budget.
    for candidate in [LoginType::Type1, LoginType::Type2, LoginType::Type3] {
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    let probes = candidates
        .into_iter()
        .map(|candidate| probe_login_type(compatibility, candidate, route_context));
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
    if let IpAddr::V6(ipv6) = address {
        if ipv6.is_unspecified()
            || ipv6.is_loopback()
            || ipv6.is_unicast_link_local()
            || ipv6.is_multicast()
            || ipv6.to_ipv4_mapped().is_some()
        {
            return Err("有线登录地址发现接口未返回可用的客户端 IPv6 地址".to_string());
        }
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
        "callback=726427262624&lang=6c7e3b7578&program_index=79225954737327212323222f212e2723&page_index=755e577b7c4e27212323222f212e2320&user_account=&wlan_user_ip=&wlan_user_ipv6=&wlan_user_mac=262626262626262626262626&jsVersion={}&encrypt=1&v={}&lang=zh",
        eportal_encrypt(LGN_JS_VERSION),
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
    eportal_client: &Client,
    user: &str,
    pass: &str,
    compatibility: VpnCompatibility,
    route_context: Option<&PortalRouteContext>,
) -> Result<(bool, String), String> {
    let local_ipv4 = login_source_ipv4(route_context, "172.30.201.2:802", "lgn 有线")?;
    // The captured browser performs a read-only getipv6 request, then exactly
    // one encrypted ePortal login with both addresses. /V6 and legacy form
    // posts are not part of this protocol and can create a separate session.
    let (observed_ipv6, diagnostic) =
        match discover_lgn_ipv6(compatibility, Duration::from_secs(5), route_context).await {
            Ok((address, source)) => (address, format!("已提交 IPv4+IPv6 联动认证（{source}）")),
            Err(error) => (
                String::new(),
                format!("IPv6 地址发现失败，已提交单 IPv4 认证：{error}"),
            ),
        };
    let login_url = lgn_login_url(user, pass, &local_ipv4, &observed_ipv6)?;
    match get_jsonp_login(
        eportal_client,
        login_url,
        LGN_REFERER,
        "lgn 有线登录",
        None,
        None,
    )
    .await
    {
        Ok((success, message)) => Ok((success, format!("{message}；{diagnostic}"))),
        // Once credentials have been submitted, an unreadable response must
        // never trigger an IPv4 retry or a second authentication protocol.
        Err(error) if login_result_is_ambiguous(&error) => Err(error),
        Err(error) => Err(format!("{AMBIGUOUS_LOGIN_RESULT}：{error}；{diagnostic}")),
    }
}

async fn discover_lgn_ipv6(
    compatibility: VpnCompatibility,
    timeout: Duration,
    route_context: Option<&PortalRouteContext>,
) -> Result<(String, &'static str), String> {
    let client = lgn_ipv6_client(compatibility, timeout, route_context)?;
    tokio::time::timeout(timeout, fetch_lgn_observed_ipv6(&client))
        .await
        .map_err(|_| "IPv6 地址发现超时".to_string())?
}

async fn fetch_lgn_observed_ipv6(client: &Client) -> Result<(String, &'static str), String> {
    match fetch_lgn_observed_ipv6_jsonp(client).await {
        Ok(address) => Ok((address, "getipv6 JSONP")),
        Err(jsonp_error) => fetch_lgn_observed_ipv6_from_page(client)
            .await
            .map(|address| (address, "lgn6 登录页 v46ip 回退"))
            .map_err(|page_error| {
                format!("getipv6 接口失败：{jsonp_error}；lgn6 登录页回退失败：{page_error}")
            }),
    }
}

pub(crate) async fn diagnose_lgn_ipv6_rust(
    compatibility: VpnCompatibility,
    route_context: Option<&PortalRouteContext>,
) -> Result<String, String> {
    discover_lgn_ipv6(compatibility, NETWORK_PROBE_TIMEOUT, route_context)
        .await
        .map(|(_address, source)| {
            format!("可用（{source}）；已取得 IPv4+IPv6 联动认证所需的 IPv6 地址")
        })
}

async fn fetch_lgn_observed_ipv6_jsonp(client: &Client) -> Result<String, String> {
    let response = client
        .get(lgn_observed_ipv6_url()?)
        .header(ACCEPT, "*/*")
        .header(REFERER, LGN_REFERER)
        .header(CACHE_CONTROL, "no-cache, no-store")
        .send()
        .await
        .map_err(|error| portal_request_error("lgn IPv6 地址发现", error, None))?;
    if !response.status().is_success() {
        return Err(format!(
            "lgn IPv6 地址发现接口返回 HTTP {}",
            response.status()
        ));
    }
    let body = response.text().await.map_err(|error| {
        format!(
            "lgn IPv6 地址发现响应读取失败：{}",
            redact_request_error(error)
        )
    })?;
    parse_observed_ip(&body, true)
}

async fn fetch_lgn_observed_ipv6_from_page(client: &Client) -> Result<String, String> {
    let response = client
        .get(LGN6_ROOT)
        .header(ACCEPT, "text/html,*/*;q=0.8")
        .header(REFERER, LGN_REFERER)
        .header(CACHE_CONTROL, "no-cache, no-store")
        .send()
        .await
        .map_err(|error| portal_request_error("lgn6 登录页 IPv6 回退", error, None))?;
    if !response.status().is_success() {
        return Err(format!("lgn6 登录页返回 HTTP {}", response.status()));
    }
    let body = response
        .text()
        .await
        .map_err(|error| format!("lgn6 登录页读取失败：{}", redact_request_error(error)))?;
    parse_lgn_page_ipv6(&body)
}

fn parse_lgn_page_ipv6(text: &str) -> Result<String, String> {
    for field in ["v46ip", "myv6ip"] {
        let Some((_, remainder)) = text.split_once(&format!("{field}=")) else {
            continue;
        };
        let remainder = remainder.trim_start();
        let Some(quote) = remainder
            .chars()
            .next()
            .filter(|character| matches!(character, '\'' | '"'))
        else {
            continue;
        };
        let value = remainder[quote.len_utf8()..]
            .split(quote)
            .next()
            .unwrap_or_default()
            .trim();
        let Ok(address) = value.parse::<Ipv6Addr>() else {
            continue;
        };
        if is_bjut_client_ipv6(&address) {
            return Ok(address.to_string());
        }
    }
    Err("登录页未包含有效的 BJUT 客户端 IPv6 地址".to_string())
}

fn is_bjut_client_ipv6(address: &Ipv6Addr) -> bool {
    let octets = address.octets();
    !address.is_unspecified() && octets[..6] == [0x20, 0x01, 0x0d, 0xa8, 0x02, 0x16]
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
    // LGN submits both addresses to HTTPS ePortal, including in Maximum
    // mode. IPv6 discovery uses a separate connector on the same campus link.
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
        LoginType::Type3 => login_lgn_once(&client, user, pass, compatibility, route_context).await,
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
            let (observed_ipv6, diagnostic) =
                match discover_lgn_ipv6(compatibility, Duration::from_secs(5), route_context).await
                {
                    Ok((address, _)) => (address, "已提交 IPv4+IPv6 联动注销".to_string()),
                    Err(error) => (
                        String::new(),
                        format!("IPv6 地址发现失败，已提交单 IPv4 注销：{error}"),
                    ),
                };
            get_jsonp_login(
                &client,
                lgn_logout_url(&local_ipv4, &observed_ipv6)?,
                LGN_REFERER,
                "lgn 注销",
                None,
                None,
            )
            .await
            .map(|(success, message)| (success, format!("{message}；{diagnostic}")))
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

    fn lgn_har_fixture() -> Value {
        serde_json::from_str(include_str!("portal_auth/fixtures/lgn-dual-stack.json")).unwrap()
    }

    #[test]
    fn lgn_requests_and_responses_match_the_sanitized_har() {
        let fixture = lgn_har_fixture();
        let user = fixture["user"].as_str().unwrap();
        let password = fixture["password"].as_str().unwrap();
        let ipv4 = fixture["ipv4"].as_str().unwrap();
        let ipv6 =
            parse_observed_ip(fixture["discovery"]["response"].as_str().unwrap(), true).unwrap();
        assert_eq!(ipv6, fixture["ipv6"].as_str().unwrap());
        let requests = [
            ("discovery", lgn_observed_ipv6_url().unwrap()),
            ("login", lgn_login_url(user, password, ipv4, &ipv6).unwrap()),
            ("logout", lgn_logout_url(ipv4, &ipv6).unwrap()),
        ];
        for (name, actual) in requests {
            let expected = Url::parse(fixture[name]["url"].as_str().unwrap()).unwrap();
            assert_eq!(actual.origin(), expected.origin(), "{name} origin");
            assert_eq!(actual.path(), expected.path(), "{name} path");
            let fields = |url: &Url| {
                url.query_pairs()
                    .into_owned()
                    .filter(|(key, _)| key != "v")
                    .collect::<Vec<_>>()
            };
            // Compare the captured wire values, including empty fields and
            // both lang entries, without reusing eportal_encrypt as an oracle.
            assert_eq!(fields(&actual), fields(&expected), "{name} query");
            assert!(!actual.as_str().contains(password));
        }
        for (name, message) in [
            ("login", "Portal协议认证成功！"),
            ("logout", "Portal协议注销成功！"),
        ] {
            assert_eq!(
                parse_dr_response(fixture[name]["response"].as_str().unwrap()).unwrap(),
                (true, message.to_string())
            );
        }
    }

    #[test]
    fn lgn_ipv4_fallback_keeps_an_empty_ipv6_field() {
        let login = lgn_login_url("25000000", "test-password", "192.0.2.10", "").unwrap();
        let pairs: std::collections::HashMap<_, _> = login.query_pairs().into_owned().collect();
        assert_eq!(pairs.get("wlan_user_ipv6").map(String::as_str), Some(""));
        assert_eq!(
            pairs.get("wlan_user_ip").map(String::as_str),
            Some("272f2438263824382726")
        );
        assert_eq!(pairs.get("login_ip_type").map(String::as_str), Some("26"));
        assert_eq!(pairs.get("encrypt").map(String::as_str), Some("1"));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn lgn_ipv6_connector_uses_the_selected_interface_without_an_ipv4_source_bind() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        use std::thread;
        use std::time::Instant;

        let fixture = lgn_har_fixture();
        let response_body = fixture["discovery"]["response"]
            .as_str()
            .unwrap()
            .to_string();
        let listener = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        assert!(peer.is_ipv6());
                        break stream;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "no IPv6 connection arrived");
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("IPv6 accept failed: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = String::new();
            let mut reader = BufReader::new(&mut stream);
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                request.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(), response_body
            ).unwrap();
            request
        });
        let interface = if cfg!(target_os = "macos") {
            "lo0"
        } else {
            "lo"
        };
        // A synthetic, nonlocal IPv4 proves that the discovery connector does
        // not accidentally bind the IPv4 source supplied for ePortal login.
        let route = PortalRouteContext::new(interface, "192.0.2.10").unwrap();
        let missing_route = PortalRouteContext::new("bjut-no-if", "192.0.2.10").unwrap();
        let mut url = Url::parse(&format!("http://{address}/drcom/getipv6")).unwrap();
        url.set_query(lgn_observed_ipv6_url().unwrap().query());
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let body = runtime.block_on(async {
            let wrong_interface = lgn_ipv6_client(
                VpnCompatibility::Minimum,
                Duration::from_secs(2),
                Some(&missing_route),
            )
            .unwrap();
            assert!(wrong_interface.get(url.clone()).send().await.is_err());
            let client =
                lgn_ipv6_client(VpnCompatibility::High, Duration::from_secs(2), Some(&route))
                    .unwrap();
            client.get(url).send().await.unwrap().text().await.unwrap()
        });
        assert_eq!(parse_observed_ip(&body, true).unwrap(), "2001:db8::10");
        let request = server.join().unwrap();
        assert!(request.starts_with("GET /drcom/getipv6?callback=dr1004&"));
        assert!(!request.contains("user_account"));
        assert!(!request.contains("user_password"));
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn lgn_ipv6_discovery_requires_a_physical_route() {
        assert!(bind_lgn_ipv6_route(Client::builder(), None).is_err());
    }

    #[test]
    fn lgn6_fixed_resolver_uses_the_observed_ipv6_gateways() {
        assert_eq!(
            LGN6_GATEWAY_IPV6.map(|address| address.to_string()),
            [
                "2001:da8:216:30c9::2".to_string(),
                "2001:da8:216:30c9::a".to_string(),
            ]
        );
    }

    #[test]
    fn observed_ip_jsonp_requires_the_expected_address_family() {
        assert_eq!(
            parse_observed_ip(r#"dr1004({"result":1,"ip":"2001:db8::10"});"#, true).unwrap(),
            "2001:db8::10"
        );
        assert!(parse_observed_ip(r#"dr1004({"result":1,"ip":"172.30.0.1"});"#, true).is_err());
        assert!(parse_observed_ip(r#"dr1004({"result":0,"ip":""});"#, true).is_err());
        for address in [
            "::",
            "::1",
            "fe80::1",
            "ff02::1",
            "::ffff:192.0.2.1",
            "invalid",
        ] {
            let body = format!(r#"dr1004({{"result":1,"ip":"{address}"}});"#);
            assert!(parse_observed_ip(&body, true).is_err(), "{address}");
        }
    }

    #[test]
    fn lgn_landing_page_can_supply_the_client_ipv6_fallback() {
        let html = r#"<script>v6='[2001:da8:216:30c9::a]'; myv6ip=' '; v46ip='2001:da8:216:2633:5067:13:669b:506c';</script>"#;
        assert_eq!(
            parse_lgn_page_ipv6(html).unwrap(),
            "2001:da8:216:2633:5067:13:669b:506c"
        );
        assert!(parse_lgn_page_ipv6("<html>no address</html>").is_err());
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
            login_probe_candidates("bjut_wifi", "wifi", None),
            vec![LoginType::Type2, LoginType::Type1]
        );
        assert_eq!(
            login_probe_candidates("bjut-sushe--5G-bEY5", "wifi", None),
            vec![LoginType::Type1, LoginType::Type2]
        );
        assert_eq!(
            login_probe_candidates("CU_bjut-sushe-28Au", "wifi", None),
            vec![LoginType::Type1, LoginType::Type2]
        );
        assert_eq!(
            login_probe_candidates("bjut_wifi", "ethernet", None),
            vec![LoginType::Type1, LoginType::Type3]
        );
        assert_eq!(
            login_probe_candidates("untrusted-lookalike", "unknown", None),
            vec![LoginType::Type1, LoginType::Type2, LoginType::Type3]
        );
        let lgn_route = PortalRouteContext::new("en5", "172.26.33.104").unwrap();
        assert_eq!(
            login_probe_candidates("", "ethernet", Some(&lgn_route)),
            vec![LoginType::Type3, LoginType::Type1]
        );
        let dorm_wired = PortalRouteContext::new("en5", "10.126.80.236").unwrap();
        assert_eq!(
            login_probe_candidates("", "ethernet", Some(&dorm_wired)),
            vec![LoginType::Type1, LoginType::Type3]
        );
    }

    #[test]
    fn simultaneous_gateways_follow_physical_network_priority() {
        fn select_login_type_detection(
            candidates: Vec<LoginType>,
            results: Vec<PortalProbeResult>,
        ) -> LoginTypeDetection {
            let probes = candidates
                .into_iter()
                .zip(results)
                .map(|(candidate, result)| {
                    std::future::ready(LoginTypeDetection::from_probe(candidate, result))
                });
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(select_prioritized_probes(probes))
        }
        let both_ready = vec![PortalProbeResult::LoginReady, PortalProbeResult::LoginReady];
        assert_eq!(
            select_login_type_detection(
                vec![LoginType::Type2, LoginType::Type1],
                both_ready.clone(),
            )
            .login_type,
            LoginType::Type2
        );
        assert_eq!(
            select_login_type_detection(
                vec![LoginType::Type1, LoginType::Type2],
                both_ready.clone(),
            )
            .login_type,
            LoginType::Type1
        );
        assert_eq!(
            select_login_type_detection(
                vec![LoginType::Type3, LoginType::Type1],
                both_ready.clone(),
            )
            .login_type,
            LoginType::Type3
        );
        assert_eq!(
            select_login_type_detection(vec![LoginType::Type1, LoginType::Type3], both_ready,)
                .login_type,
            LoginType::Type1
        );
        let preferred_portal_only = select_login_type_detection(
            vec![LoginType::Type1, LoginType::Type3],
            vec![
                PortalProbeResult::PortalDetected,
                PortalProbeResult::LoginReady,
            ],
        );
        assert_eq!(preferred_portal_only.login_type, LoginType::Type1);
        assert!(preferred_portal_only.portal_detected);
        assert!(!preferred_portal_only.login_ready);
    }

    #[test]
    fn preferred_gateway_does_not_wait_for_unreachable_secondary_gateways() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let probes: Vec<BoxFuture<'_, LoginTypeDetection>> = vec![
                async {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    LoginTypeDetection::from_probe(LoginType::Type3, PortalProbeResult::LoginReady)
                }
                .boxed(),
                std::future::pending().boxed(),
            ];
            let result = tokio::time::timeout(
                Duration::from_millis(500),
                select_prioritized_probes(probes),
            )
            .await
            .unwrap();
            assert_eq!(result.login_type, LoginType::Type3);
        });
    }

    #[test]
    fn fast_secondary_gateway_does_not_override_physical_link_priority() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let probes: Vec<BoxFuture<'_, LoginTypeDetection>> = vec![
                async {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    LoginTypeDetection::from_probe(
                        LoginType::Type1,
                        PortalProbeResult::PortalDetected,
                    )
                }
                .boxed(),
                std::future::ready(LoginTypeDetection::from_probe(
                    LoginType::Type3,
                    PortalProbeResult::LoginReady,
                ))
                .boxed(),
            ];
            let result = select_prioritized_probes(probes).await;
            assert_eq!(result.login_type, LoginType::Type1);
            assert!(!result.login_ready);
        });
    }

    #[test]
    fn gateway_timeouts_preserve_partial_evidence_and_do_not_block_ipv6() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let partial = finish_portal_probes(
                LoginType::Type1,
                vec![
                    std::future::ready(PortalProbeResult::PortalDetected).boxed(),
                    std::future::pending().boxed(),
                ],
                Duration::from_millis(30),
            )
            .await;
            assert!(partial.portal_detected && partial.timed_out && !partial.login_ready);
            let ipv6 = finish_portal_probes(
                LoginType::Type3,
                vec![
                    std::future::pending().boxed(),
                    std::future::ready(PortalProbeResult::LoginReady).boxed(),
                ],
                Duration::from_millis(30),
            )
            .await;
            assert!(ipv6.login_ready && !ipv6.timed_out);
        });
    }

    #[test]
    fn type3_verified_portal_allows_ipv4_fallback_without_ipv6() {
        let mut evidence = Type3ProbeEvidence::default();
        evidence.record(
            "https://lgn.bjut.edu.cn:802/eportal/portal/page/loadUserInfo",
            r#"dr1002({"code":1,"user_info":{"account":"25000000"}});"#,
        );
        assert_eq!(evidence.result(), PortalProbeResult::LoginReady);

        evidence.record(
            "https://lgn6.bjut.edu.cn/drcom/getipv6",
            r#"dr1004({"result":0,"ip":""});"#,
        );
        assert_eq!(evidence.result(), PortalProbeResult::LoginReady);

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
        assert_eq!(readiness.len(), 5);
        assert_eq!(readiness[0], LGN_ROOT);
        assert_eq!(readiness[1], LGN6_ROOT);
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
        assert!(probe_body_matches(
            &LoginType::Type3,
            LGN_ROOT,
            r#"<!--Dr.COMWebLoginID_0.htm--><script>authuserfield='DDDDD';</script>"#,
        ));
        assert!(!probe_body_matches(
            &LoginType::Type1,
            "http://10.21.221.98:801/eportal/portal/login",
            "generic reverse proxy error page",
        ));
    }
}
