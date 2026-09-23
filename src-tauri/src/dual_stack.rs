use crate::network_probe::NETWORK_PROBE_TIMEOUT;
use futures_util::{stream::FuturesUnordered, FutureExt, StreamExt};
#[cfg(test)]
use std::net::IpAddr;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FamilyConnectivity {
    pub(crate) addresses: Vec<String>,
    pub(crate) status: String,
    pub(crate) detail: String,
    pub(crate) duration_ms: u128,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DualStackReport {
    pub(crate) interface_name: String,
    pub(crate) checked_at: String,
    #[serde(default)]
    pub(crate) scope: String,
    #[serde(default)]
    pub(crate) generation: u64,
    #[serde(default)]
    pub(crate) probe_id: u64,
    pub(crate) ipv4: FamilyConnectivity,
    pub(crate) ipv6: FamilyConnectivity,
}

impl DualStackReport {
    pub(crate) fn redact_addresses(&mut self) {
        for (family, placeholder) in [
            (&mut self.ipv4, "[LOCAL-IPv4]"),
            (&mut self.ipv6, "[LOCAL-IPv6]"),
        ] {
            for address in &mut family.addresses {
                family.detail = family.detail.replace(address.as_str(), placeholder);
                *address = placeholder.to_string();
            }
        }
    }
    pub(crate) fn online(&self) -> bool {
        self.ipv4.status == "reachable" || self.ipv6.status == "reachable"
    }
    pub(crate) fn invalidate(&mut self, reason: &str) {
        for family in [&mut self.ipv4, &mut self.ipv6] {
            family.status = "unknown".to_string();
            family.detail = reason.to_string();
        }
    }
}

#[derive(Clone, Copy)]
enum Family {
    V4,
    V6,
}
impl Family {
    #[cfg(test)]
    fn matches(self, address: IpAddr) -> bool {
        matches!(
            (self, address),
            (Self::V4, IpAddr::V4(_)) | (Self::V6, IpAddr::V6(_))
        )
    }
}

#[derive(Clone, Copy)]
enum ResponseCheck {
    NoContent,
    #[cfg(test)]
    Address,
    Apple,
    Microsoft,
}

#[derive(Debug)]
pub(crate) struct ProbeFailure {
    pub(crate) kind: &'static str,
    pub(crate) detail: String,
}
impl ProbeFailure {
    fn response(detail: impl Into<String>) -> Self {
        Self {
            kind: "response_error",
            detail: detail.into(),
        }
    }
    fn timeout() -> Self {
        Self {
            kind: "timeout",
            detail: "3 秒内未完成响应".into(),
        }
    }
}
async fn check_target(
    network: &serde_json::Value,
    direct: bool,
    family: Family,
    url: &str,
    check: ResponseCheck,
) -> Result<(), ProbeFailure> {
    let (status, body) =
        crate::probe_transport::request(network, direct, matches!(family, Family::V6), url).await?;
    if matches!(check, ResponseCheck::NoContent) {
        return if status == 204 {
            Ok(())
        } else {
            Err(ProbeFailure::response(format!(
                "HTTP {status}，响应不是 204（可能被认证页拦截）"
            )))
        };
    }
    if status != 200 {
        return Err(ProbeFailure::response(format!("HTTP {status}")));
    }
    let body = String::from_utf8_lossy(&body);
    let valid = match check {
        ResponseCheck::Microsoft => body.trim() == "Microsoft Connect Test",
        ResponseCheck::Apple => {
            body.contains("<TITLE>Success</TITLE>") && body.contains("<BODY>Success</BODY>")
        }
        #[cfg(test)]
        ResponseCheck::Address => body
            .trim()
            .parse::<IpAddr>()
            .is_ok_and(|ip| family.matches(ip) && !ip.is_unspecified() && !ip.is_loopback()),
        ResponseCheck::NoContent => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ProbeFailure::response(
            "响应内容或地址族不符合预期（可能被认证页拦截）",
        ))
    }
}

async fn probe_family(
    network: &serde_json::Value,
    direct: bool,
    family: Family,
    addresses: Vec<String>,
    targets: &[(&str, ResponseCheck)],
) -> FamilyConnectivity {
    let started = std::time::Instant::now();
    let mut errors = Vec::new();
    {
        let mut probes: FuturesUnordered<_> = targets
            .iter()
            .map(|(url, check)| async move {
                let result = tokio::time::timeout(
                    NETWORK_PROBE_TIMEOUT,
                    check_target(network, direct, family, url, *check),
                )
                .await
                .unwrap_or_else(|_| Err(ProbeFailure::timeout()));
                (url, result)
            })
            .collect();
        while let Some((url, result)) = probes.next().await {
            match result {
                Ok(()) => {
                    return FamilyConnectivity {
                        addresses,
                        status: "reachable".to_string(),
                        detail: format!(
                            "{}外网响应校验通过（{}）",
                            if direct {
                                "网卡直连"
                            } else {
                                "系统路由"
                            },
                            reqwest::Url::parse(url)
                                .ok()
                                .and_then(|url| url.host_str().map(str::to_string))
                                .unwrap_or_default()
                        ),
                        duration_ms: started.elapsed().as_millis(),
                    }
                }
                Err(mut error) => {
                    if let Ok(url) = reqwest::Url::parse(url) {
                        error.detail =
                            format!("{}：{}", url.host_str().unwrap_or("目标"), error.detail);
                    }
                    errors.push(error);
                }
            }
        }
    }
    let status = if errors.iter().any(|error| error.kind == "timeout") {
        "timeout"
    } else if errors
        .first()
        .is_some_and(|first| errors.iter().all(|error| error.kind == first.kind))
    {
        errors.first().map(|error| error.kind).unwrap_or("unknown")
    } else {
        "unknown"
    };
    FamilyConnectivity {
        addresses,
        status: status.into(),
        detail: format!(
            "{}。探测未通过不等于该地址族不可用。",
            errors
                .iter()
                .map(|error| error.detail.as_str())
                .collect::<Vec<_>>()
                .join("；")
        ),
        duration_ms: started.elapsed().as_millis(),
    }
}

fn family_unconfigured(adapters: &[crate::network_inventory::NetworkAdapter], ipv6: bool) -> bool {
    // An empty/failed inventory is unknown, not proof of absent configuration.
    let connected: Vec<_> = adapters
        .iter()
        .filter(|adapter| adapter.connected)
        .collect();
    !connected.is_empty()
        && connected.iter().all(|adapter| {
            if ipv6 {
                adapter.ipv6.is_empty()
            } else {
                adapter.ipv4.is_empty()
            }
        })
}

pub(crate) fn unavailable(reason: &str) -> DualStackReport {
    let family = FamilyConnectivity {
        addresses: Vec::new(),
        status: "unknown".into(),
        detail: reason.into(),
        duration_ms: 0,
    };
    DualStackReport {
        interface_name: String::new(),
        checked_at: chrono::Local::now().to_rfc3339(),
        scope: "system".into(),
        generation: 0,
        probe_id: 0,
        ipv4: family.clone(),
        ipv6: family,
    }
}

fn physical_identity_matches(
    adapter: &crate::network_inventory::NetworkAdapter,
    network: &serde_json::Value,
    android: bool,
) -> bool {
    let ip = network["ip"].as_str().unwrap_or("");
    // UI selection support is independent of the ability to use a physical Network.
    adapter.connected
        && matches!(adapter.transport.as_str(), "wifi" | "ethernet")
        && adapter.interface_name == network["interfaceName"].as_str().unwrap_or("")
        && (ip.is_empty() || adapter.ipv4.iter().any(|candidate| candidate == ip))
        && (!android
            || network["networkId"]
                .as_str()
                .is_some_and(|id| id == adapter.id))
}

#[cfg(any(target_os = "android", test))]
fn android_snapshot_adapter(
    network: &serde_json::Value,
) -> Option<crate::network_inventory::NetworkAdapter> {
    let transport = network["transport"].as_str()?;
    let interface = network["interfaceName"].as_str()?;
    let id = network["networkId"].as_str()?;
    let handle = network["physicalNetworkHandle"]
        .as_str()?
        .parse::<u64>()
        .ok()?;
    if !matches!(transport, "wifi" | "ethernet")
        || interface.is_empty()
        || id.is_empty()
        || handle == 0
    {
        return None;
    }
    let addresses = |key: &str, ipv6: bool| -> Option<Vec<String>> {
        Some(
            network[key]
                .as_array()?
                .iter()
                .filter_map(|value| value.as_str())
                .filter(|value| {
                    if ipv6 {
                        crate::network_inventory::usable_ipv6(value)
                    } else {
                        crate::usable_physical_ipv4(value).is_some()
                    }
                })
                .map(str::to_string)
                .collect(),
        )
    };
    Some(crate::network_inventory::NetworkAdapter {
        id: id.into(),
        interface_name: interface.into(),
        transport: transport.into(),
        ipv4: addresses("ipv4", false)?,
        ipv6: addresses("ipv6", true)?,
        connected: true,
        ..Default::default()
    })
}

pub(crate) async fn probe_with_updates(
    network: &serde_json::Value,
    update: impl FnMut(&DualStackReport),
) -> DualStackReport {
    probe_route_with_updates(network, false, update).await
}

pub(crate) async fn probe_route_with_updates(
    network: &serde_json::Value,
    direct: bool,
    mut update: impl FnMut(&DualStackReport),
) -> DualStackReport {
    let interface = network["interfaceName"].as_str().unwrap_or("");
    #[cfg(not(target_os = "android"))]
    let adapters = crate::network_inventory::adapters();
    #[cfg(target_os = "android")]
    let adapters = if direct {
        android_snapshot_adapter(network).into_iter().collect()
    } else {
        crate::network_inventory::adapters()
    };
    let adapter = adapters
        .iter()
        .find(|adapter| physical_identity_matches(adapter, network, cfg!(target_os = "android")));
    let ipv4 = adapter
        .map(|adapter| adapter.ipv4.clone())
        .unwrap_or_default();
    let ipv6 = adapter
        .map(|adapter| adapter.ipv6.clone())
        .unwrap_or_default();
    let mut route = network.clone();
    if direct {
        let mut servers = adapter
            .map(|adapter| adapter.dns_servers.clone())
            .unwrap_or_default();
        #[cfg(target_os = "macos")]
        if adapter.is_some()
            && network["lgnLinkConfiguration"]["dnsServers"]
                .as_array()
                .is_none_or(Vec::is_empty)
        {
            // Wired snapshots already include scoped DNS. Read Wi-Fi DHCP and
            // scoped resolver data once per manual diagnostic, not every poll.
            servers.extend(crate::macos_network::lgn_link_configuration(interface).dns_servers);
        }
        servers.extend(
            network["dnsServers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|value| value.as_str().map(str::to_string)),
        );
        route["dnsServers"] = serde_json::json!(servers);
    }
    let network = &route;
    let pending = |addresses| FamilyConnectivity {
        addresses,
        status: "checking".to_string(),
        detail: "正在探测".to_string(),
        duration_ms: 0,
    };
    let mut report = DualStackReport {
        interface_name: interface.to_string(),
        checked_at: chrono::Local::now().to_rfc3339(),
        scope: if direct { "physical" } else { "system" }.to_string(),
        generation: 0,
        probe_id: 0,
        ipv4: pending(ipv4.clone()),
        ipv6: pending(ipv6.clone()),
    };
    let v4_targets = [
        (
            "http://www.msftconnecttest.com/connecttest.txt",
            ResponseCheck::Microsoft,
        ),
        (
            "http://captive.apple.com/hotspot-detect.html",
            ResponseCheck::Apple,
        ),
        (
            "https://cp.cloudflare.com/generate_204",
            ResponseCheck::NoContent,
        ),
    ];
    let v6_targets = [
        (
            "http://ipv6.msftconnecttest.com/connecttest.txt",
            ResponseCheck::Microsoft,
        ),
        (
            "http://captive.apple.com/hotspot-detect.html",
            ResponseCheck::Apple,
        ),
        (
            "https://cp.cloudflare.com/generate_204",
            ResponseCheck::NoContent,
        ),
    ];
    if direct && adapter.is_none() {
        report.ipv4.status = "unavailable".into();
        report.ipv6.status = "unavailable".into();
        report.ipv4.detail = "未取得认证网卡，请刷新后重试".into();
        report.ipv6.detail = report.ipv4.detail.clone();
        update(&report);
        return report;
    }
    let missing_v4 = if direct {
        ipv4.is_empty()
    } else {
        family_unconfigured(&adapters, false)
    };
    let missing_v6 = if direct {
        ipv6.is_empty()
    } else {
        family_unconfigured(&adapters, true)
    };
    let absent = |addresses| FamilyConnectivity {
        addresses,
        status: "not_configured".into(),
        detail: "已读取网络接口，当前连接未配置此地址族的可用地址；无需据此重新认证".into(),
        duration_ms: 0,
    };
    let mut probes = FuturesUnordered::new();
    if missing_v4 {
        report.ipv4 = absent(ipv4);
    } else {
        probes.push(
            async {
                (
                    false,
                    probe_family(network, direct, Family::V4, ipv4, &v4_targets).await,
                )
            }
            .boxed(),
        );
    }
    if missing_v6 {
        report.ipv6 = absent(ipv6);
    } else {
        probes.push(
            async {
                (
                    true,
                    probe_family(network, direct, Family::V6, ipv6, &v6_targets).await,
                )
            }
            .boxed(),
        );
    }
    update(&report);
    while let Some((is_v6, result)) = probes.next().await {
        if is_v6 {
            report.ipv6 = result;
        } else {
            report.ipv4 = result;
        }
        report.checked_at = chrono::Local::now().to_rfc3339();
        update(&report);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_os = "android"))]
    #[test]
    #[ignore = "requires a configured, online physical network; sends no credentials"]
    fn live_system_and_physical_probes() {
        let interface = std::env::var("BJUT_PROBE_INTERFACE")
            .expect("set BJUT_PROBE_INTERFACE to the physical interface to test");
        let adapter = crate::network_inventory::adapters()
            .into_iter()
            .find(|adapter| adapter.interface_name == interface && adapter.connected)
            .expect("connected physical interface");
        let network = serde_json::json!({"interfaceName":interface, "ip":adapter.ipv4.first().cloned().unwrap_or_default()});
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (system, physical) = futures_util::future::join(
                probe_with_updates(&network, |_| {}),
                probe_route_with_updates(&network, true, |_| {}),
            )
            .await;
            for report in [system, physical] {
                for (family, result) in [("IPv4", report.ipv4), ("IPv6", report.ipv6)] {
                    eprintln!(
                        "{} {family}: {} ({} ms) {}",
                        report.scope, result.status, result.duration_ms, result.detail
                    );
                    if family == "IPv4" || std::env::var_os("BJUT_PROBE_EXPECT_IPV6").is_some() {
                        assert_eq!(result.status, "reachable");
                    }
                }
            }
        });
    }

    #[test]
    fn response_validation_rejects_captive_redirects_and_accepts_chunked_expected_body() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/", server.local_addr().unwrap());
            let worker = tokio::spawn(async move {
                for response in [
                    "HTTP/1.1 302 Found\r\nContent-Length: 0\r\nLocation: /login\r\n\r\n",
                    "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nlogin",
                    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n16\r\nMicrosoft Connect Test\r\n0\r\n\r\n",
                ] {
                    let (mut socket, _) = server.accept().await.unwrap();
                    let mut request = [0; 2048]; assert!(socket.read(&mut request).await.unwrap() > 0);
                    socket.write_all(response.as_bytes()).await.unwrap();
                }
            });
            for expected in [false, false, true] {
                let result = tokio::time::timeout(NETWORK_PROBE_TIMEOUT, check_target(&serde_json::Value::Null, false, Family::V4, &url, ResponseCheck::Microsoft)).await.unwrap();
                assert_eq!(result.is_ok(), expected, "{result:?}");
            }
            worker.await.unwrap();
        });
    }
    #[test]
    fn android_physical_network_does_not_require_manual_selection_support() {
        let mut adapter = crate::network_inventory::NetworkAdapter {
            id: "101".into(),
            interface_name: "wlan0".into(),
            transport: "wifi".into(),
            connected: true,
            selectable: false,
            ipv4: vec!["192.168.1.216".into()],
            ..Default::default()
        };
        let network =
            serde_json::json!({"interfaceName":"wlan0", "ip":"192.168.1.216", "networkId":"101"});
        assert!(physical_identity_matches(&adapter, &network, true));
        adapter.id = "102".into();
        assert!(!physical_identity_matches(&adapter, &network, true));
        adapter.id = "101".into();
        adapter.transport = "vpn".into();
        assert!(!physical_identity_matches(&adapter, &network, true));
        adapter.transport = "wifi".into();
        adapter.connected = false;
        assert!(!physical_identity_matches(&adapter, &network, true));
    }
    #[test]
    fn android_direct_probes_use_the_native_snapshot_without_an_activity_inventory() {
        let mut snapshot = serde_json::json!({
            "networkId":"101", "physicalNetworkHandle":"10100", "interfaceName":"wlan0", "transport":"wifi",
            "ip":"192.168.1.2", "ipv4":["192.168.1.2", "198.18.0.1"], "ipv6":["2001:db8::1", "fe80::1"]
        });
        let adapter = android_snapshot_adapter(&snapshot).unwrap();
        assert_eq!(adapter.ipv4, ["192.168.1.2"]);
        assert_eq!(adapter.ipv6, ["2001:db8::1"]);
        assert!(physical_identity_matches(&adapter, &snapshot, true));
        snapshot["physicalNetworkHandle"] = "".into();
        assert!(android_snapshot_adapter(&snapshot).is_none());
        snapshot["physicalNetworkHandle"] = "10100".into();
        snapshot["transport"] = "vpn".into();
        assert!(android_snapshot_adapter(&snapshot).is_none());
    }
    #[test]
    fn absent_family_requires_inventory_and_includes_other_interfaces() {
        use crate::network_inventory::NetworkAdapter;
        assert!(!family_unconfigured(&[], true));
        let v4 = NetworkAdapter {
            connected: true,
            ipv4: vec!["192.0.2.42".into()],
            ..Default::default()
        };
        assert!(family_unconfigured(std::slice::from_ref(&v4), true));
        let v6 = NetworkAdapter {
            connected: true,
            ipv6: vec!["2001:db8::42".into()],
            ..Default::default()
        };
        assert!(!family_unconfigured(&[v4, v6], true));
    }
    #[test]
    fn diagnostic_bundle_redacts_both_address_families() {
        let family = |ip: &str| FamilyConnectivity {
            addresses: vec![ip.to_string()],
            status: "reachable".to_string(),
            detail: format!("source {ip}"),
            duration_ms: 1,
        };
        let mut report = DualStackReport {
            interface_name: "en7".to_string(),
            checked_at: String::new(),
            scope: "system".to_string(),
            generation: 0,
            probe_id: 0,
            ipv4: family("172.26.99.10"),
            ipv6: family("2001:db8::10"),
        };
        report.redact_addresses();
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("172.26.99.10") && !serialized.contains("2001:db8::10"));
        assert!(serialized.contains("[LOCAL-IPv6]"));
    }
    #[cfg(unix)]
    #[test]
    fn ipv4_success_does_not_mark_failed_ipv6_as_reachable() {
        use std::io::{Read, Write};
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut connection, peer) = server.accept().unwrap();
            assert!(peer.is_ipv4());
            let mut buffer = [0u8; 4096];
            let count = connection.read(&mut buffer).unwrap();
            assert!(std::str::from_utf8(&buffer[..count])
                .unwrap()
                .starts_with("GET /generate_204"));
            connection
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });
        let target = format!("http://{address}/generate_204");
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (ipv4, ipv6) = futures_util::future::join(
                probe_family(
                    &serde_json::Value::Null,
                    false,
                    Family::V4,
                    vec!["192.0.2.99".to_string()],
                    &[(&target, ResponseCheck::NoContent)],
                ),
                probe_family(
                    &serde_json::Value::Null,
                    false,
                    Family::V6,
                    vec!["2001:db8::99".to_string()],
                    &[(&target, ResponseCheck::NoContent)],
                ),
            )
            .await;
            assert_eq!(ipv4.status, "reachable");
            assert_ne!(ipv6.status, "reachable");
        });
        worker.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn ipv6_probe_uses_system_source_and_checks_the_returned_family() {
        use std::io::{Read, Write};
        let server = std::net::TcpListener::bind("[::1]:0").unwrap();
        let address = server.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            for body in ["2001:db8::42", "192.0.2.42"] {
                let (mut connection, peer) = server.accept().unwrap();
                assert!(peer.is_ipv6());
                let mut buffer = [0u8; 4096];
                let count = connection.read(&mut buffer).unwrap();
                assert!(count > 0);
                write!(
                    connection,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let target = format!("http://{address}/ip");
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            for expected in ["reachable", "response_error"] {
                let result = probe_family(
                    &serde_json::Value::Null,
                    false,
                    Family::V6,
                    vec!["2001:db8::99".to_string()],
                    &[(&target, ResponseCheck::Address)],
                )
                .await;
                assert_eq!(result.status, expected);
            }
        });
        worker.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn successful_target_finishes_without_waiting_for_a_stalled_peer() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let fast = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let slow = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let fast_url = format!("http://{}/", fast.local_addr().unwrap());
            let slow_url = format!("http://{}/", slow.local_addr().unwrap());
            let fast_worker = tokio::spawn(async move {
                let (mut connection, _) = fast.accept().await.unwrap();
                let mut buffer = [0u8; 4096];
                let count = connection.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                connection.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
            });
            let slow_worker = tokio::spawn(async move {
                let (connection, _) = slow.accept().await.unwrap();
                std::future::pending::<()>().await;
                drop(connection);
            });
            let result = tokio::time::timeout(std::time::Duration::from_secs(2), probe_family(&serde_json::Value::Null, false, Family::V4, Vec::new(), &[(&slow_url, ResponseCheck::NoContent), (&fast_url, ResponseCheck::NoContent)])).await;
            slow_worker.abort();
            fast_worker.await.unwrap();
            assert_eq!(result.expect("a stalled target must not delay a valid response").status, "reachable");
        });
    }
}
