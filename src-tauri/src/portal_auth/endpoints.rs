//! Single source of truth for every campus gateway endpoint the app dials.
//! Defaults are the values captured from the live portal flows; P1-02 will let
//! users override them, so nothing outside this module may hardcode them.
//!
//! The captured URL constants remain as golden values for tests; production
//! builds the same strings from the resolved IP so a user override only changes
//! the dial target, never the protocol contract (scheme/port/path).
#![allow(dead_code)]

use crate::config_model::EndpointOverrides;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

// ---- bjut-sushe (dorm, ePortal on 10.21.221.98) ----
pub(crate) const DORM_GATEWAY_IPV4: Ipv4Addr = Ipv4Addr::new(10, 21, 221, 98);
pub(crate) const DORM_HTTP_LOGIN: &str = "http://10.21.221.98:801/eportal/portal/login";
pub(crate) const DORM_HTTP_AUTHORITY: &str = "10.21.221.98:801";
pub(crate) const DORM_HTTP_REFERER: &str = "http://10.21.221.98/";
pub(crate) const DORM_HTTPS_AUTHORITY: &str = "10.21.221.98:802";
pub(crate) const DORM_HTTPS_REFERER: &str = "https://10.21.221.98:802/";

// ---- bjut_wifi (Dr.COM on 10.21.251.3 / wlgn) ----
pub(crate) const WIFI_GATEWAY_IPV4: Ipv4Addr = Ipv4Addr::new(10, 21, 251, 3);
pub(crate) const WIFI_ROUTE_PROBE: &str = "10.21.251.3:80";
pub(crate) const WLGN_HOST: &str = "wlgn.bjut.edu.cn";
pub(crate) const WIFI_HTTP_LOGIN: &str = "http://10.21.251.3/drcom/login";
pub(crate) const WIFI_HTTP_LOGOUT: &str = "http://10.21.251.3/drcom/logout";
pub(crate) const WIFI_HTTP_CHKSTATUS: &str = "http://10.21.251.3/drcom/chkstatus";
pub(crate) const WIFI_HTTP_REFERER: &str = "http://10.21.251.3/";
pub(crate) const WIFI_HTTPS_LOGIN: &str = "https://wlgn.bjut.edu.cn/drcom/login";
pub(crate) const WIFI_HTTPS_LOGOUT: &str = "https://wlgn.bjut.edu.cn/drcom/logout";
pub(crate) const WIFI_HTTPS_CHKSTATUS: &str = "https://wlgn.bjut.edu.cn/drcom/chkstatus";
pub(crate) const WIFI_HTTPS_REFERER: &str = "https://wlgn.bjut.edu.cn/";

// ---- lgn (wired, 172.30.201.2 / 172.30.201.10 + lgn6) ----
pub(crate) const LGN_HOST: &str = "lgn.bjut.edu.cn";
pub(crate) const LGN6_HOST: &str = "lgn6.bjut.edu.cn";
#[cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(dead_code))]
pub(crate) const LGN_IPV4_PRIMARY: &str = "172.30.201.2";
pub(crate) const LGN_IPV4_GATEWAYS: [Ipv4Addr; 2] = [
    Ipv4Addr::new(172, 30, 201, 2),
    Ipv4Addr::new(172, 30, 201, 10),
];
pub(crate) const LGN6_GATEWAY_IPV6: [Ipv6Addr; 2] = [
    Ipv6Addr::new(0x2001, 0x0da8, 0x0216, 0x30c9, 0, 0, 0, 0x0002),
    Ipv6Addr::new(0x2001, 0x0da8, 0x0216, 0x30c9, 0, 0, 0, 0x000a),
];
pub(crate) const LGN_HTTPS_AUTHORITY: &str = "172.30.201.2:802";
pub(crate) const LGN_HTTPS_LOGIN: &str = "https://lgn.bjut.edu.cn:802/eportal/portal/login";
pub(crate) const LGN_HTTPS_LOGOUT: &str = "https://lgn.bjut.edu.cn:802/eportal/portal/logout";
pub(crate) const LGN_HTTP_USER_INFO: &str =
    "http://172.30.201.2:801/eportal/portal/page/loadUserInfo";
pub(crate) const LGN_HTTPS_USER_INFO: &str =
    "https://lgn.bjut.edu.cn:802/eportal/portal/page/loadUserInfo";
pub(crate) const LGN6_OBSERVED_IPV6: &str = "https://lgn6.bjut.edu.cn/drcom/getipv6";
pub(crate) const LGN_REFERER: &str = "https://lgn.bjut.edu.cn/";
pub(crate) const LGN_ROOT: &str = "https://lgn.bjut.edu.cn/";
pub(crate) const LGN6_ROOT: &str = "https://lgn6.bjut.edu.cn/";

/// Official BJUT hostnames used as TLS SNI/Host aliases while DNS is pinned to
/// the dorm gateway. Limited to hostnames already present in captured traffic.
pub(crate) const TLS_HOST_CANDIDATES: [&str; 2] = [WLGN_HOST, LGN_HOST];

/// Resolved gateway addresses for one operation. Built once from config at the
/// operation boundary, then passed by reference; no global mutable state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GatewayEndpoints {
    dorm_ipv4: Ipv4Addr,
    wifi_ipv4: Ipv4Addr,
    lgn_ipv4: Ipv4Addr,
    lgn6_ipv6: Ipv6Addr,
}

impl Default for GatewayEndpoints {
    fn default() -> Self {
        Self::resolve(&EndpointOverrides::default())
    }
}

impl GatewayEndpoints {
    /// Pure overlay of user overrides on the built-in values. Invalid values
    /// fall back to the built-in address: saving rejects them, so this is only
    /// a defensive layer for imported or older data.
    pub(crate) fn resolve(overrides: &EndpointOverrides) -> Self {
        Self {
            dorm_ipv4: parse_ipv4(&overrides.dorm_gateway, DORM_GATEWAY_IPV4),
            wifi_ipv4: parse_ipv4(&overrides.wifi_gateway, WIFI_GATEWAY_IPV4),
            lgn_ipv4: parse_ipv4(&overrides.lgn_gateway, LGN_IPV4_GATEWAYS[0]),
            lgn6_ipv6: parse_ipv6(&overrides.lgn6_gateway, LGN6_GATEWAY_IPV6[0]),
        }
    }

    pub(crate) fn dorm_ipv4(&self) -> Ipv4Addr {
        self.dorm_ipv4
    }

    pub(crate) fn wifi_ipv4(&self) -> Ipv4Addr {
        self.wifi_ipv4
    }

    pub(crate) fn lgn_ipv4(&self) -> Ipv4Addr {
        self.lgn_ipv4
    }

    pub(crate) fn lgn6_ipv6(&self) -> Ipv6Addr {
        self.lgn6_ipv6
    }

    /// Primary address follows the override; the captured secondary address is
    /// kept as a built-in fallback.
    pub(crate) fn lgn_ipv4_gateways(&self) -> [Ipv4Addr; 2] {
        [self.lgn_ipv4(), LGN_IPV4_GATEWAYS[1]]
    }

    pub(crate) fn lgn6_gateways(&self) -> [Ipv6Addr; 2] {
        [self.lgn6_ipv6(), LGN6_GATEWAY_IPV6[1]]
    }

    pub(crate) fn dorm_http_login(&self) -> String {
        format!("http://{}:801/eportal/portal/login", self.dorm_ipv4)
    }

    pub(crate) fn dorm_http_authority(&self) -> String {
        format!("{}:801", self.dorm_ipv4)
    }

    pub(crate) fn dorm_http_referer(&self) -> String {
        format!("http://{}/", self.dorm_ipv4)
    }

    pub(crate) fn dorm_https_authority(&self) -> String {
        format!("{}:802", self.dorm_ipv4)
    }

    pub(crate) fn dorm_https_referer(&self) -> String {
        format!("https://{}:802/", self.dorm_ipv4)
    }

    pub(crate) fn wifi_http_login(&self) -> String {
        format!("http://{}/drcom/login", self.wifi_ipv4)
    }

    pub(crate) fn wifi_http_logout(&self) -> String {
        format!("http://{}/drcom/logout", self.wifi_ipv4)
    }

    pub(crate) fn wifi_http_chkstatus(&self) -> String {
        format!("http://{}/drcom/chkstatus", self.wifi_ipv4)
    }

    pub(crate) fn wifi_http_referer(&self) -> String {
        format!("http://{}/", self.wifi_ipv4)
    }

    // The HTTPS URLs keep the official hostname: the override only changes the
    // pinned address table, not the certificate-bound SNI/Host.
    pub(crate) fn wifi_https_login(&self) -> String {
        WIFI_HTTPS_LOGIN.to_string()
    }

    pub(crate) fn wifi_https_logout(&self) -> String {
        WIFI_HTTPS_LOGOUT.to_string()
    }

    pub(crate) fn wifi_https_chkstatus(&self) -> String {
        WIFI_HTTPS_CHKSTATUS.to_string()
    }

    pub(crate) fn wifi_https_referer(&self) -> String {
        WIFI_HTTPS_REFERER.to_string()
    }

    pub(crate) fn lgn_http_user_info(&self) -> String {
        format!(
            "http://{}:801/eportal/portal/page/loadUserInfo",
            self.lgn_ipv4
        )
    }

    pub(crate) fn lgn_https_authority(&self) -> String {
        format!("{}:802", self.lgn_ipv4)
    }

    pub(crate) fn lgn6_observed_ipv6(&self) -> String {
        LGN6_OBSERVED_IPV6.to_string()
    }
}

fn parse_ipv4(value: &str, fallback: Ipv4Addr) -> Ipv4Addr {
    value.trim().parse().unwrap_or(fallback)
}

fn parse_ipv6(value: &str, fallback: Ipv6Addr) -> Ipv6Addr {
    value.trim().parse().unwrap_or(fallback)
}

#[derive(Clone, Copy)]
enum AddressFamily {
    V4,
    V6,
}

/// Validate user-editable gateway overrides. An empty value means "use the
/// built-in address"; everything else must be a usable unicast IP literal.
pub(crate) fn validate_endpoint_overrides(overrides: &EndpointOverrides) -> Result<(), String> {
    validate_gateway("宿舍网关", &overrides.dorm_gateway, AddressFamily::V4)?;
    validate_gateway("Wi-Fi 网关", &overrides.wifi_gateway, AddressFamily::V4)?;
    validate_gateway("有线网关", &overrides.lgn_gateway, AddressFamily::V4)?;
    validate_gateway("有线 IPv6 网关", &overrides.lgn6_gateway, AddressFamily::V6)
}

fn validate_gateway(label: &str, value: &str, family: AddressFamily) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    if value.trim() != value {
        return Err(format!("{label} IP 地址前后不能有空格"));
    }
    if value.contains("://") {
        return Err(format!("{label}只填写 IP 地址，不要带 http:// 或 https://"));
    }
    if value.contains('/') || value.contains('?') || value.contains('#') {
        return Err(format!("{label}只填写 IP 地址，不要带路径或查询参数"));
    }
    if value.contains('@') {
        return Err(format!("{label}只填写 IP 地址，不能包含账号或密码"));
    }
    let address: IpAddr = match value.parse() {
        Ok(address) => address,
        Err(_) => {
            if value.chars().any(|ch| ch.is_ascii_alphabetic()) {
                return Err(format!(
                    "{label}必须填写 IP 地址：域名与证书绑定，不接受修改"
                ));
            }
            return Err(format!("{label}不是合法的 IP 地址"));
        }
    };
    match (family, address) {
        (AddressFamily::V4, IpAddr::V4(address)) => validate_ipv4(label, address),
        (AddressFamily::V6, IpAddr::V6(address)) => validate_ipv6(label, address),
        (AddressFamily::V4, IpAddr::V6(_)) => Err(format!("{label}必须是 IPv4 地址")),
        (AddressFamily::V6, IpAddr::V4(_)) => Err(format!("{label}必须是 IPv6 地址")),
    }
}

fn validate_ipv4(label: &str, address: Ipv4Addr) -> Result<(), String> {
    if address.is_unspecified()
        || address.is_loopback()
        || address.is_multicast()
        || address.is_broadcast()
        || address.is_link_local()
    {
        return Err(format!("{label}不能是未指定、回环或组播地址"));
    }
    Ok(())
}

fn validate_ipv6(label: &str, address: Ipv6Addr) -> Result<(), String> {
    if address.is_unspecified()
        || address.is_loopback()
        || address.is_multicast()
        || address.is_unicast_link_local()
        || address.to_ipv4_mapped().is_some()
    {
        return Err(format!("{label}不能是未指定、回环或组播地址"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_constants_are_the_captured_gateway_values() {
        // A literal snapshot: any endpoint change must update this test, so a
        // silent typo cannot point the app at the wrong gateway.
        assert_eq!(
            DORM_HTTP_LOGIN,
            "http://10.21.221.98:801/eportal/portal/login"
        );
        assert_eq!(DORM_HTTP_AUTHORITY, "10.21.221.98:801");
        assert_eq!(DORM_HTTP_REFERER, "http://10.21.221.98/");
        assert_eq!(DORM_HTTPS_AUTHORITY, "10.21.221.98:802");
        assert_eq!(DORM_HTTPS_REFERER, "https://10.21.221.98:802/");
        assert_eq!(WIFI_HTTP_LOGIN, "http://10.21.251.3/drcom/login");
        assert_eq!(WIFI_HTTP_LOGOUT, "http://10.21.251.3/drcom/logout");
        assert_eq!(WIFI_HTTP_CHKSTATUS, "http://10.21.251.3/drcom/chkstatus");
        assert_eq!(WIFI_HTTPS_LOGIN, "https://wlgn.bjut.edu.cn/drcom/login");
        assert_eq!(WIFI_HTTPS_LOGOUT, "https://wlgn.bjut.edu.cn/drcom/logout");
        assert_eq!(
            WIFI_HTTPS_CHKSTATUS,
            "https://wlgn.bjut.edu.cn/drcom/chkstatus"
        );
        assert_eq!(
            LGN_HTTPS_LOGIN,
            "https://lgn.bjut.edu.cn:802/eportal/portal/login"
        );
        assert_eq!(
            LGN_HTTPS_LOGOUT,
            "https://lgn.bjut.edu.cn:802/eportal/portal/logout"
        );
        assert_eq!(
            LGN_HTTP_USER_INFO,
            "http://172.30.201.2:801/eportal/portal/page/loadUserInfo"
        );
        assert_eq!(
            LGN_HTTPS_USER_INFO,
            "https://lgn.bjut.edu.cn:802/eportal/portal/page/loadUserInfo"
        );
        assert_eq!(LGN6_OBSERVED_IPV6, "https://lgn6.bjut.edu.cn/drcom/getipv6");
        assert_eq!(LGN_REFERER, "https://lgn.bjut.edu.cn/");
        assert_eq!(LGN_ROOT, "https://lgn.bjut.edu.cn/");
        assert_eq!(LGN6_ROOT, "https://lgn6.bjut.edu.cn/");
        assert_eq!(WIFI_ROUTE_PROBE, "10.21.251.3:80");
        assert_eq!(LGN_IPV4_PRIMARY, "172.30.201.2");
        assert_eq!(LGN_HTTPS_AUTHORITY, "172.30.201.2:802");

        // IP constants and URL constants must agree with each other.
        assert_eq!(DORM_GATEWAY_IPV4.to_string(), "10.21.221.98");
        assert!(DORM_HTTP_LOGIN.contains(&DORM_GATEWAY_IPV4.to_string()));
        assert!(DORM_HTTPS_REFERER.contains(&DORM_GATEWAY_IPV4.to_string()));
        assert_eq!(WIFI_GATEWAY_IPV4.to_string(), "10.21.251.3");
        assert!(WIFI_HTTP_LOGIN.contains(&WIFI_GATEWAY_IPV4.to_string()));
        assert_eq!(
            LGN_IPV4_GATEWAYS.map(|address| address.to_string()),
            ["172.30.201.2".to_string(), "172.30.201.10".to_string()]
        );
        assert!(LGN_HTTP_USER_INFO.contains(&LGN_IPV4_GATEWAYS[0].to_string()));
        assert_eq!(
            LGN6_GATEWAY_IPV6.map(|address| address.to_string()),
            [
                "2001:da8:216:30c9::2".to_string(),
                "2001:da8:216:30c9::a".to_string()
            ]
        );
        assert_eq!(TLS_HOST_CANDIDATES, [WLGN_HOST, LGN_HOST]);
    }

    #[test]
    fn gateway_overrides_default_to_the_built_in_endpoints() {
        let endpoints = GatewayEndpoints::default();
        assert_eq!(endpoints.dorm_ipv4(), DORM_GATEWAY_IPV4);
        assert_eq!(endpoints.wifi_ipv4(), WIFI_GATEWAY_IPV4);
        assert_eq!(endpoints.lgn_ipv4(), LGN_IPV4_GATEWAYS[0]);
        assert_eq!(endpoints.lgn6_ipv6(), LGN6_GATEWAY_IPV6[0]);
        assert_eq!(endpoints.lgn_ipv4_gateways(), LGN_IPV4_GATEWAYS);
        assert_eq!(endpoints.lgn6_gateways(), LGN6_GATEWAY_IPV6);

        assert_eq!(endpoints.dorm_http_login(), DORM_HTTP_LOGIN);
        assert_eq!(endpoints.dorm_http_authority(), DORM_HTTP_AUTHORITY);
        assert_eq!(endpoints.dorm_http_referer(), DORM_HTTP_REFERER);
        assert_eq!(endpoints.dorm_https_authority(), DORM_HTTPS_AUTHORITY);
        assert_eq!(endpoints.dorm_https_referer(), DORM_HTTPS_REFERER);
        assert_eq!(endpoints.wifi_http_login(), WIFI_HTTP_LOGIN);
        assert_eq!(endpoints.wifi_http_logout(), WIFI_HTTP_LOGOUT);
        assert_eq!(endpoints.wifi_http_chkstatus(), WIFI_HTTP_CHKSTATUS);
        assert_eq!(endpoints.wifi_http_referer(), WIFI_HTTP_REFERER);
        assert_eq!(endpoints.wifi_https_login(), WIFI_HTTPS_LOGIN);
        assert_eq!(endpoints.wifi_https_logout(), WIFI_HTTPS_LOGOUT);
        assert_eq!(endpoints.wifi_https_chkstatus(), WIFI_HTTPS_CHKSTATUS);
        assert_eq!(endpoints.wifi_https_referer(), WIFI_HTTPS_REFERER);
        assert_eq!(endpoints.lgn_http_user_info(), LGN_HTTP_USER_INFO);
        assert_eq!(endpoints.lgn_https_authority(), LGN_HTTPS_AUTHORITY);
        assert_eq!(endpoints.lgn6_observed_ipv6(), LGN6_OBSERVED_IPV6);
    }

    #[test]
    fn gateway_overrides_replace_each_dial_target() {
        let overrides = EndpointOverrides {
            dorm_gateway: "10.0.0.9".into(),
            wifi_gateway: "10.0.0.10".into(),
            lgn_gateway: "10.0.0.11".into(),
            lgn6_gateway: "2001:db8::9".into(),
        };
        let endpoints = GatewayEndpoints::resolve(&overrides);
        assert_eq!(endpoints.dorm_ipv4(), Ipv4Addr::new(10, 0, 0, 9));
        assert_eq!(endpoints.wifi_ipv4(), Ipv4Addr::new(10, 0, 0, 10));
        assert_eq!(endpoints.lgn_ipv4(), Ipv4Addr::new(10, 0, 0, 11));
        assert_eq!(
            endpoints.lgn6_ipv6(),
            "2001:db8::9".parse::<Ipv6Addr>().unwrap()
        );
        assert_eq!(
            endpoints.lgn_ipv4_gateways(),
            [Ipv4Addr::new(10, 0, 0, 11), LGN_IPV4_GATEWAYS[1]]
        );
        assert_eq!(
            endpoints.lgn6_gateways(),
            [
                "2001:db8::9".parse::<Ipv6Addr>().unwrap(),
                LGN6_GATEWAY_IPV6[1]
            ]
        );

        assert_eq!(
            endpoints.dorm_http_login(),
            "http://10.0.0.9:801/eportal/portal/login"
        );
        assert_eq!(endpoints.wifi_http_login(), "http://10.0.0.10/drcom/login");
        assert_eq!(
            endpoints.lgn_http_user_info(),
            "http://10.0.0.11:801/eportal/portal/page/loadUserInfo"
        );
        // HTTPS URLs stay on the certificate-bound hostname; only the pinned
        // address table changes.
        assert_eq!(endpoints.wifi_https_login(), WIFI_HTTPS_LOGIN);
        assert_eq!(endpoints.lgn6_observed_ipv6(), LGN6_OBSERVED_IPV6);
    }

    #[test]
    fn clearing_a_gateway_override_restores_the_built_in_value() {
        let mut overrides = EndpointOverrides {
            dorm_gateway: "10.0.0.9".into(),
            wifi_gateway: "10.0.0.10".into(),
            lgn_gateway: "10.0.0.11".into(),
            lgn6_gateway: "2001:db8::9".into(),
        };
        assert_ne!(
            GatewayEndpoints::resolve(&overrides).dorm_ipv4(),
            DORM_GATEWAY_IPV4
        );
        overrides.dorm_gateway.clear();
        overrides.wifi_gateway.clear();
        overrides.lgn_gateway.clear();
        overrides.lgn6_gateway.clear();
        assert!(overrides.is_empty());
        assert_eq!(
            GatewayEndpoints::resolve(&overrides),
            GatewayEndpoints::default()
        );
    }

    #[test]
    fn gateway_overrides_reject_scheme_path_port_and_credentials() {
        for value in [
            "http://10.0.0.1",
            "10.0.0.1:8080",
            "10.0.0.1/portal",
            "user@10.0.0.1",
            " 10.0.0.1",
            "10.0.0.1 ",
        ] {
            let overrides = EndpointOverrides {
                dorm_gateway: value.into(),
                ..EndpointOverrides::default()
            };
            let error = validate_endpoint_overrides(&overrides)
                .expect_err("invalid gateway override must be rejected");
            assert!(error.contains("IP"), "{value}: {error}");
        }
    }

    #[test]
    fn gateway_overrides_reject_domains_with_an_explanation() {
        for value in ["wlgn.bjut.edu.cn", "portal.example.com"] {
            let overrides = EndpointOverrides {
                dorm_gateway: value.into(),
                ..EndpointOverrides::default()
            };
            let error = validate_endpoint_overrides(&overrides)
                .expect_err("a domain must never be accepted as a dial target");
            assert!(error.contains("域名"), "{value}: {error}");
            assert!(error.contains("证书"), "{value}: {error}");
        }
    }

    #[test]
    fn gateway_overrides_reject_the_wrong_address_family() {
        let overrides = EndpointOverrides {
            lgn6_gateway: "172.30.201.2".into(),
            ..EndpointOverrides::default()
        };
        let error = validate_endpoint_overrides(&overrides).expect_err("IPv4 is not lgn6");
        assert!(error.contains("IPv6"), "{error}");

        let overrides = EndpointOverrides {
            dorm_gateway: "2001:db8::1".into(),
            ..EndpointOverrides::default()
        };
        let error = validate_endpoint_overrides(&overrides).expect_err("IPv6 is not dorm");
        assert!(error.contains("IPv4"), "{error}");
    }

    #[test]
    fn gateway_overrides_reject_loopback_unspecified_and_multicast() {
        for value in [
            "0.0.0.0",
            "127.0.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "169.254.1.1",
        ] {
            let overrides = EndpointOverrides {
                dorm_gateway: value.into(),
                ..EndpointOverrides::default()
            };
            assert!(
                validate_endpoint_overrides(&overrides).is_err(),
                "{value} must be rejected"
            );
        }
        for value in ["::", "::1", "ff02::1", "fe80::1", "::ffff:10.0.0.1"] {
            let overrides = EndpointOverrides {
                lgn6_gateway: value.into(),
                ..EndpointOverrides::default()
            };
            assert!(
                validate_endpoint_overrides(&overrides).is_err(),
                "{value} must be rejected"
            );
        }
    }
}
