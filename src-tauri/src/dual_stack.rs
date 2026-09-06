use crate::network_inventory::NetworkAdapter;
use crate::network_probe::NETWORK_PROBE_TIMEOUT;
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

async fn probe_family(
    interface: &str,
    addresses: Vec<String>,
    targets: &[&str],
) -> FamilyConnectivity {
    let started = std::time::Instant::now();
    let Some(source) = addresses
        .first()
        .and_then(|address| address.parse::<IpAddr>().ok())
    else {
        return FamilyConnectivity {
            addresses,
            status: "not_configured".to_string(),
            detail: "当前认证网卡未取得此类可用地址".to_string(),
            duration_ms: 0,
        };
    };
    // The local address restricts DNS results to one family. Explicit interface
    // binding and no_proxy prevent another adapter/proxy masking a broken link.
    let builder = reqwest::Client::builder()
        .no_proxy()
        .local_address(source)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(NETWORK_PROBE_TIMEOUT)
        .use_rustls_tls();
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let builder = builder.interface(interface);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = interface;
    let results = match builder.build() {
        Ok(client) => {
            futures_util::future::join_all(targets.iter().map(|url| {
                let client = &client;
                async move {
                    match tokio::time::timeout(
                        NETWORK_PROBE_TIMEOUT,
                        client
                            .get(*url)
                            .header("Cache-Control", "no-cache, no-store")
                            .send(),
                    )
                    .await
                    {
                        Ok(Ok(response))
                            if response.status() == reqwest::StatusCode::NO_CONTENT =>
                        {
                            Ok(())
                        }
                        Ok(Ok(response)) => Err(format!(
                            "HTTP {}，未取得预期的 204 响应",
                            response.status().as_u16()
                        )),
                        Ok(Err(error)) => Err(error.without_url().to_string()),
                        Err(_) => Err("3 秒内无响应".to_string()),
                    }
                }
            }))
            .await
        }
        Err(error) => vec![Err(error.without_url().to_string())],
    };
    let reachable = results.iter().any(Result::is_ok);
    FamilyConnectivity {
        addresses,
        status: if reachable {
            "reachable"
        } else {
            "unreachable"
        }
        .to_string(),
        detail: if reachable {
            "通过此网卡独立连接外网，响应校验通过".to_string()
        } else {
            format!(
                "此网卡的外网探测未通过；可能涉及 DNS、路由或认证。{}",
                results
                    .into_iter()
                    .filter_map(Result::err)
                    .collect::<Vec<_>>()
                    .join("；")
            )
        },
        duration_ms: started.elapsed().as_millis(),
    }
}

pub(crate) async fn probe(network: &serde_json::Value) -> DualStackReport {
    let interface = network["interfaceName"].as_str().unwrap_or("");
    let ip = network["ip"].as_str().unwrap_or("");
    let mut adapter = crate::network_inventory::adapters()
        .into_iter()
        .find(|adapter| {
            adapter.interface_name == interface
                && adapter.connected
                && adapter.ipv4.iter().any(|candidate| candidate == ip)
        });
    if let Some(adapter) = &mut adapter {
        adapter.ipv4 = vec![ip.to_string()];
        adapter
            .ipv6
            .sort_by_key(|value| !value.starts_with("2001:da8:216:"));
    }
    let mut report = probe_adapter(interface, adapter.as_ref()).await;
    if adapter.is_none() {
        report.invalidate("未取得与当前认证身份一致的接口信息，请重新检测");
    }
    report
}

async fn probe_adapter(interface: &str, adapter: Option<&NetworkAdapter>) -> DualStackReport {
    let targets = [
        "https://cp.cloudflare.com/generate_204",
        "https://connectivitycheck.gstatic.com/generate_204",
    ];
    let ipv4 = adapter
        .map(|adapter| adapter.ipv4.clone())
        .unwrap_or_default();
    let ipv6 = adapter
        .map(|adapter| adapter.ipv6.clone())
        .unwrap_or_default();
    let (ipv4, ipv6) = futures_util::future::join(
        probe_family(interface, ipv4, &targets),
        probe_family(interface, ipv6, &targets),
    )
    .await;
    DualStackReport {
        interface_name: interface.to_string(),
        checked_at: chrono::Local::now().to_rfc3339(),
        ipv4,
        ipv6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let interface = if cfg!(target_os = "macos") {
            "lo0"
        } else {
            "lo"
        };
        let target = format!("http://{address}/generate_204");
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (ipv4, ipv6) = futures_util::future::join(
                probe_family(interface, vec!["127.0.0.1".to_string()], &[&target]),
                probe_family(interface, vec!["::1".to_string()], &[&target]),
            )
            .await;
            assert_eq!(ipv4.status, "reachable");
            assert_eq!(ipv6.status, "unreachable");
            assert_eq!(
                probe_family(interface, vec![], &[&target]).await.status,
                "not_configured"
            );
        });
        worker.join().unwrap();
    }
}
