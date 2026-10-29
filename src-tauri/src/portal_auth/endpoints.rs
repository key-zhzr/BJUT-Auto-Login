//! Single source of truth for every campus gateway endpoint the app dials.
//! Defaults are the values captured from the live portal flows; P1-02 will let
//! users override them, so nothing outside this module may hardcode them.
use std::net::{Ipv4Addr, Ipv6Addr};

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
#[cfg_attr(
    not(any(target_os = "macos", target_os = "linux")),
    allow(dead_code)
)]
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
}
