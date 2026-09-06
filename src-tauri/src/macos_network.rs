//! Shared macOS physical-link selection and interface-scoped IPv6 diagnostics.

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PhysicalIdentity {
    pub(crate) interface: String,
    pub(crate) ipv4: String,
    pub(crate) transport: &'static str,
}

fn select_identity(
    wifi: Option<(String, String)>,
    routed_wired: Option<(String, String)>,
    campus_wired: Option<(String, String)>,
) -> Option<PhysicalIdentity> {
    let is_campus = |candidate: &(String, String)| {
        !candidate.0.is_empty()
            && crate::usable_physical_ipv4(&candidate.1).is_some()
            && crate::network_trust::is_campus_local_ip(&candidate.1)
    };
    routed_wired
        .filter(is_campus)
        .or_else(|| campus_wired.filter(is_campus))
        .map(|(interface, ipv4)| PhysicalIdentity {
            interface,
            ipv4,
            transport: "ethernet",
        })
        .or_else(|| {
            wifi.filter(|(interface, ip)| {
                !interface.is_empty() && crate::usable_physical_ipv4(ip).is_some()
            })
            .map(|(interface, ipv4)| PhysicalIdentity {
                interface,
                ipv4,
                transport: "wifi",
            })
        })
}

/// This never reads SSID/BSSID. Both the periodic IP poll and full observation
/// must use this selector, so simultaneous Wi-Fi cannot alternate the baseline.
#[cfg(target_os = "macos")]
pub(crate) fn physical_identity() -> Option<PhysicalIdentity> {
    use crate::network_platform::*;
    let wifi_interface = corewlan::WiFiClient::shared()
        .ok()
        .and_then(|client| client.interface())
        .and_then(|interface| interface.interface_name())
        .unwrap_or_default();
    let wifi_ip = macos_ipv4_for_interface(&wifi_interface);
    let route_interface = macos_route_interface("172.30.201.2");
    let routed_wired = if route_interface != wifi_interface
        && macos_is_physical_ethernet_interface(&route_interface)
    {
        let ip = macos_ipv4_for_interface(&route_interface);
        Some((route_interface, ip))
    } else {
        None
    };
    let campus_wired = if routed_wired
        .as_ref()
        .is_some_and(|(_, ip)| is_campus_wired_ipv4(ip))
    {
        None
    } else {
        macos_campus_wired_identity(&wifi_interface)
    };
    select_identity(Some((wifi_interface, wifi_ip)), routed_wired, campus_wired)
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LgnLinkConfiguration {
    pub(crate) ipv6_configured: bool,
    pub(crate) dns_scoped: bool,
    pub(crate) dns_servers: Vec<String>,
    pub(crate) search_domains: Vec<String>,
    pub(crate) gateway: String,
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !value.is_empty() && !values.contains(&value) {
        values.push(value);
    }
}

fn parse_link_configuration(
    interface: &str,
    ifconfig: &str,
    dhcp: &str,
    dns: &str,
) -> LgnLinkConfiguration {
    let ipv6_configured = ifconfig.lines().any(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        fields.first() == Some(&"inet6")
            && !fields
                .iter()
                .any(|field| ["tentative", "duplicated", "detached", "deprecated"].contains(field))
            && fields
                .get(1)
                .and_then(|address| address.split('%').next())
                .and_then(|address| address.parse::<std::net::Ipv6Addr>().ok())
                .is_some_and(|address| address.segments()[..3] == [0x2001, 0xda8, 0x216])
    });
    let mut result = LgnLinkConfiguration {
        ipv6_configured,
        ..Default::default()
    };
    for resolver in dns.split("resolver #") {
        // Never combine another adapter's DNS/search domains with this link.
        let on_interface = resolver.lines().any(|line| {
            line.trim_start().starts_with("if_index")
                && line
                    .split_once('(')
                    .and_then(|(_, rest)| rest.split_once(')'))
                    .is_some_and(|(name, _)| name.trim() == interface)
        });
        if !on_interface {
            continue;
        }
        result.dns_scoped = true;
        for line in resolver.lines() {
            let Some((key, value)) = line.trim().split_once(':') else {
                continue;
            };
            let value = value.trim().to_ascii_lowercase();
            if key.starts_with("nameserver[")
                && value
                    .split('%')
                    .next()
                    .is_some_and(|ip| ip.parse::<std::net::IpAddr>().is_ok())
            {
                push_unique(&mut result.dns_servers, value);
            } else if key.starts_with("search domain[") || key.trim() == "domain" {
                push_unique(
                    &mut result.search_domains,
                    value.trim_end_matches('.').to_string(),
                );
            }
        }
    }
    for line in dhcp.lines().map(str::trim) {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.starts_with("router ") {
            result.gateway = value.trim().trim_matches(['{', '}']).trim().to_string();
        }
        if !result.dns_scoped && key.starts_with("domain_name_server ") {
            for address in value
                .trim()
                .trim_matches(['{', '}'])
                .split(',')
                .map(str::trim)
            {
                if address.parse::<std::net::IpAddr>().is_ok() {
                    push_unique(&mut result.dns_servers, address.to_string());
                }
            }
        }
        if !result.dns_scoped && key.starts_with("domain_name ") {
            push_unique(
                &mut result.search_domains,
                value
                    .trim()
                    .trim_matches('"')
                    .trim_end_matches('.')
                    .to_ascii_lowercase(),
            );
        }
    }
    result
}

impl LgnLinkConfiguration {
    pub(crate) fn resembles_reported_ipv6_failure(&self) -> bool {
        self.dns_scoped
            && self.ipv6_configured
            && self
                .search_domains
                .iter()
                .any(|domain| domain == "proxy.bjut.edu.cn")
            && !self
                .search_domains
                .iter()
                .any(|domain| domain == "bjut6.edu.cn")
            && !self.dns_servers.iter().any(|server| {
                server
                    .split('%')
                    .next()
                    .is_some_and(|ip| ip.parse::<std::net::Ipv6Addr>().is_ok())
            })
    }

    pub(crate) fn features(&self, ipv4: &str) -> Vec<String> {
        let mut features = Vec::new();
        if ipv4
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| matches!(ip.octets(), [172, 26, _, _]))
        {
            features.push("IPv4 172.26/16".to_string());
        }
        if self.ipv6_configured {
            features.push("IPv6 2001:da8:216::/48".to_string());
        }
        if !self.dns_servers.is_empty() {
            features.push(format!("接口 DNS {}", self.dns_servers.join(" / ")));
        }
        if !self.search_domains.is_empty() {
            features.push(format!("接口搜索域 {}", self.search_domains.join(" / ")));
        }
        if !self.gateway.is_empty() {
            features.push(format!("网关 {}", self.gateway));
        }
        features
    }

    pub(crate) fn diagnostic(&self, ipv6_available: bool) -> String {
        let mut detail = format!(
            "IPv6 地址：{}\n当前接口 DNS：{}\n当前接口搜索域：{}",
            if self.ipv6_configured {
                "已取得"
            } else {
                "未取得可用的 BJUT IPv6"
            },
            if self.dns_servers.is_empty() {
                "未取得".to_string()
            } else {
                self.dns_servers.join(" / ")
            },
            if self.search_domains.is_empty() {
                "未取得".to_string()
            } else {
                self.search_domains.join(" / ")
            }
        );
        if self.resembles_reported_ipv6_failure() {
            detail.push_str("\n仍保留 IPv6 地址，但缺少 IPv6 DNS，搜索域为 proxy.bjut.edu.cn；该组合与已报告的异常配置一致。");
        }
        detail.push_str(if ipv6_available && self.resembles_reported_ipv6_failure() {
            "\nlgn6 IPv6 地址发现通过；此探测不验证外网 IPv6。如 IPv6 上网仍异常，可重启此有线适配器以刷新配置。"
        } else if ipv6_available {
            "\nlgn6 IPv6 地址发现通过，未发现上述异常配置组合。"
        } else {
            "\nlgn6 IPv6 地址发现未通过，可重启此有线适配器以重新获取网络配置。"
        });
        detail
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn lgn_link_configuration(interface: &str) -> LgnLinkConfiguration {
    let read = |program: &str, args: &[&str]| {
        std::process::Command::new(program)
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            .unwrap_or_default()
    };
    parse_link_configuration(
        interface,
        &read("/sbin/ifconfig", &[interface]),
        &read("/usr/sbin/ipconfig", &["getpacket", interface]),
        &read("/usr/sbin/scutil", &["--dns"]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(interface: &str, ip: &str) -> Option<(String, String)> {
        Some((interface.to_string(), ip.to_string()))
    }

    #[test]
    fn simultaneous_wifi_changes_do_not_oscillate_the_wired_ip_baseline() {
        let mut previous = None;
        for wifi_ip in ["192.168.1.10", "192.168.1.11", "", "192.168.1.10"]
            .into_iter()
            .cycle()
            .take(20)
        {
            let selected =
                select_identity(link("en0", wifi_ip), None, link("en7", "172.26.99.10")).unwrap();
            assert_eq!(selected.interface, "en7");
            assert_eq!(selected.transport, "ethernet");
            assert!(!crate::network_ip_observation_changed(
                previous.as_deref(),
                &selected.ipv4
            ));
            previous = Some(selected.ipv4);
        }
        let disconnected = select_identity(link("en0", "192.168.1.10"), None, None).unwrap();
        assert_eq!(disconnected.transport, "wifi");
        assert!(crate::network_ip_observation_changed(
            previous.as_deref(),
            &disconnected.ipv4
        ));
        assert!(!crate::network_ip_observation_changed(
            Some(&disconnected.ipv4),
            "192.168.1.10"
        ));
    }

    #[test]
    fn routed_campus_ethernet_wins_and_a_real_lease_change_is_observed() {
        let first = select_identity(
            link("en0", "192.168.1.10"),
            link("en9", "172.26.88.10"),
            link("en7", "172.26.99.10"),
        )
        .unwrap();
        assert_eq!(first.interface, "en9");
        let renewed = select_identity(
            link("en0", "192.168.1.10"),
            link("en9", "172.26.88.11"),
            None,
        )
        .unwrap();
        assert!(crate::network_ip_observation_changed(
            Some(&first.ipv4),
            &renewed.ipv4
        ));
        let tun = select_identity(
            link("en0", "192.168.1.10"),
            link("utun4", "198.18.0.1"),
            link("en7", "172.26.99.10"),
        )
        .unwrap();
        assert_eq!(tun.interface, "en7");
        assert!(select_identity(link("en0", ""), None, link("en7", "")).is_none());
    }

    const IFCONFIG: &str = "en7: flags=8863<UP,BROADCAST,RUNNING>\n inet6 2001:da8:216:2699::10 prefixlen 64 autoconf secured\n status: active";
    const DHCP: &str = "domain_name_server (ip_mult): {172.21.0.21, 172.21.201.22}\ndomain_name (string): proxy.bjut.edu.cn\nrouter (ip_mult): {172.26.99.254}";

    #[test]
    fn dns_and_search_domains_are_scoped_to_the_selected_adapter() {
        let abnormal = parse_link_configuration(
            "en7",
            IFCONFIG,
            DHCP,
            r#"
resolver #1
  search domain[0] : bjut6.edu.cn
  nameserver[0] : 2001:da8:216:215d::6666
  if_index : 6 (en0)
resolver #2
  search domain[0] : proxy.bjut.edu.cn
  nameserver[0] : 172.21.0.21
  nameserver[1] : 172.21.201.22
  if_index : 20 (en7)
"#,
        );
        assert!(abnormal.ipv6_configured && abnormal.dns_scoped);
        assert!(abnormal.resembles_reported_ipv6_failure());
        assert_eq!(abnormal.search_domains, vec!["proxy.bjut.edu.cn"]);
        assert_eq!(abnormal.dns_servers, vec!["172.21.0.21", "172.21.201.22"]);
        assert!(abnormal
            .features("172.26.99.10")
            .iter()
            .any(|text| text.contains("proxy.bjut.edu.cn")));
        assert!(abnormal.diagnostic(true).contains("此探测不验证外网 IPv6"));
        assert!(abnormal.diagnostic(false).contains("地址发现未通过"));
    }

    #[test]
    fn a_valid_ipv6_dns_configuration_is_not_flagged_by_the_dhcp_search_domain() {
        let normal = parse_link_configuration(
            "en7",
            IFCONFIG,
            DHCP,
            r#"
resolver #1
  search domain[0] : bjut6.edu.cn
  nameserver[0] : 172.21.0.21
  nameserver[1] : 172.21.201.22
  nameserver[2] : 2001:da8:216:215d::6666
  if_index : 20 (en7)
"#,
        );
        assert!(!normal.resembles_reported_ipv6_failure());
        assert_eq!(normal.search_domains, vec!["bjut6.edu.cn"]);
        assert_eq!(normal.dns_servers.len(), 3);
        assert_eq!(normal.gateway, "172.26.99.254");
        let unknown =
            parse_link_configuration("en7", IFCONFIG, DHCP, "resolver #1\n if_index : 21 (en70)");
        assert!(!unknown.dns_scoped);
        assert!(!unknown.resembles_reported_ipv6_failure());
        let tentative =
            parse_link_configuration("en7", "inet6 2001:da8:216:2699::10 tentative", DHCP, "");
        assert!(!tentative.ipv6_configured);
    }
}
