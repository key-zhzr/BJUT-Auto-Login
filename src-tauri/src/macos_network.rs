//! Shared macOS physical-link selection and interface-scoped IPv6 diagnostics.

use crate::network_inventory::NetworkAdapter;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PhysicalIdentity {
    pub(crate) interface: String,
    pub(crate) ipv4: String,
    pub(crate) transport: &'static str,
}

fn select_identity(
    adapters: &[NetworkAdapter],
    route_interface: impl Fn(&str) -> String,
) -> Option<PhysicalIdentity> {
    let active = adapters.iter().filter(|adapter| {
        adapter.connected && adapter.selectable && !adapter.interface_name.is_empty()
    });
    let wired: Vec<_> = active
        .clone()
        .filter(|adapter| adapter.transport == "ethernet")
        .flat_map(|adapter| {
            adapter
                .ipv4
                .iter()
                .filter(|ip| crate::usable_physical_ipv4(ip).is_some())
                .map(move |ip| (adapter.interface_name.as_str(), ip.as_str()))
        })
        .collect();
    let campus: Vec<_> = wired
        .iter()
        .copied()
        .filter(|(_, ip)| crate::network_platform::is_campus_wired_ipv4(ip))
        .collect();
    let select_wired = |links: &[(&str, &str)], destination: &str| {
        let first = links.first()?;
        let &(interface, ipv4) = if links.len() > 1 {
            let route = route_interface(destination);
            links
                .iter()
                .find(|(name, _)| *name == route)
                .unwrap_or(first)
        } else {
            first
        };
        Some(PhysicalIdentity {
            interface: interface.to_string(),
            ipv4: ipv4.to_string(),
            transport: "ethernet",
        })
    };

    select_wired(&campus, "172.30.201.2")
        .or_else(|| {
            active
                .filter(|adapter| adapter.transport == "wifi")
                .find_map(|adapter| {
                    let ipv4 = adapter
                        .ipv4
                        .iter()
                        .find(|ip| crate::usable_physical_ipv4(ip).is_some())?;
                    Some(PhysicalIdentity {
                        interface: adapter.interface_name.clone(),
                        ipv4: ipv4.clone(),
                        transport: "wifi",
                    })
                })
        })
        // A router uplink may be reached through an ordinary Ethernet LAN even
        // when a TUN owns the campus routes and Wi-Fi has no address.
        .or_else(|| select_wired(&wired, "default"))
}

/// This never reads SSID/BSSID. Both the periodic IP poll and full observation
/// must use this selector, so simultaneous Wi-Fi cannot alternate the baseline.
#[cfg(target_os = "macos")]
pub(crate) fn physical_identity_for(preferred: &str) -> Option<PhysicalIdentity> {
    use crate::network_platform::*;
    if !preferred.is_empty() {
        let wifi = corewlan::WiFiClient::shared().ok().is_some_and(|client| {
            client
                .interface_names()
                .iter()
                .any(|name| name == preferred)
        });
        if !wifi && !macos_is_physical_ethernet_interface(preferred) {
            return None;
        }
        let ip = macos_ipv4_for_interface(preferred);
        return crate::usable_physical_ipv4(&ip).map(|_| PhysicalIdentity {
            interface: preferred.to_string(),
            ipv4: ip,
            transport: if wifi { "wifi" } else { "ethernet" },
        });
    }
    select_identity(
        &crate::network_inventory::macos_link_adapters(),
        macos_route_interface,
    )
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
    use crate::network_platform::macos_read_command;
    parse_link_configuration(
        interface,
        &macos_read_command("/sbin/ifconfig", &[interface]),
        &macos_read_command("/usr/sbin/ipconfig", &["getpacket", interface]),
        &macos_read_command("/usr/sbin/scutil", &["--dns"]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(interface: &str, transport: &str, ip: &str) -> NetworkAdapter {
        NetworkAdapter {
            interface_name: interface.to_string(),
            transport: transport.to_string(),
            ipv4: vec![ip.to_string()],
            connected: true,
            selectable: transport != "vpn",
            ..Default::default()
        }
    }

    #[test]
    fn simultaneous_wifi_changes_do_not_oscillate_the_wired_ip_baseline() {
        let mut previous = None;
        for wifi_ip in ["192.168.1.10", "192.168.1.11", "", "192.168.1.10"]
            .into_iter()
            .cycle()
            .take(20)
        {
            let selected = select_identity(
                &[
                    adapter("en0", "wifi", wifi_ip),
                    adapter("en7", "ethernet", "172.26.99.10"),
                ],
                |_| String::new(),
            )
            .unwrap();
            assert_eq!(selected.interface, "en7");
            assert_eq!(selected.transport, "ethernet");
            assert!(!crate::network_ip_observation_changed(
                previous.as_deref(),
                &selected.ipv4
            ));
            previous = Some(selected.ipv4);
        }
        let disconnected =
            select_identity(&[adapter("en0", "wifi", "192.168.1.10")], |_| String::new()).unwrap();
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
            &[
                adapter("en0", "wifi", "192.168.1.10"),
                adapter("en7", "ethernet", "172.26.99.10"),
                adapter("en9", "ethernet", "172.26.88.10"),
            ],
            |destination| {
                assert_eq!(destination, "172.30.201.2");
                "en9".to_string()
            },
        )
        .unwrap();
        assert_eq!(first.interface, "en9");
        let renewed = select_identity(
            &[
                adapter("en0", "wifi", "192.168.1.10"),
                adapter("en9", "ethernet", "172.26.88.11"),
            ],
            |_| String::new(),
        )
        .unwrap();
        assert!(crate::network_ip_observation_changed(
            Some(&first.ipv4),
            &renewed.ipv4
        ));
        let tun = select_identity(
            &[
                adapter("en0", "wifi", "192.168.1.10"),
                adapter("utun4", "vpn", "198.18.0.1"),
                adapter("en7", "ethernet", "172.26.99.10"),
                adapter("en9", "ethernet", "172.26.88.10"),
            ],
            |_| "utun4".to_string(),
        )
        .unwrap();
        assert_eq!(tun.interface, "en7");
        assert!(select_identity(
            &[adapter("en0", "wifi", ""), adapter("en7", "ethernet", "")],
            |_| String::new(),
        )
        .is_none());
    }

    #[test]
    fn ethernet_lan_remains_available_with_tun_and_no_wifi_address() {
        let links = [
            adapter("en0", "wifi", "169.254.20.3"),
            adapter("en5", "ethernet", "192.168.0.10"),
            adapter("en7", "ethernet", "192.168.1.37"),
            adapter("utun1024", "vpn", "198.18.0.1"),
        ];
        let selected = select_identity(&links, |destination| {
            assert_eq!(destination, "default");
            "en7".to_string()
        })
        .unwrap();
        assert_eq!(selected.interface, "en7");
        assert_eq!(selected.ipv4, "192.168.1.37");
        assert_eq!(selected.transport, "ethernet");

        let mut detached = links[1].clone();
        detached.connected = false;
        let selected = select_identity(&[detached, links[2].clone(), links[3].clone()], |_| {
            panic!("a single usable physical adapter needs no route lookups")
        })
        .unwrap();
        assert_eq!(selected.interface, "en7");
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
