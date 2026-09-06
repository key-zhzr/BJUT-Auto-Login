use crate::network_probe::NETWORK_PROBE_TIMEOUT;
use futures_util::{stream::FuturesUnordered, FutureExt, StreamExt};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

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
    fn source(self) -> IpAddr {
        match self {
            Self::V4 => Ipv4Addr::UNSPECIFIED.into(),
            Self::V6 => Ipv6Addr::UNSPECIFIED.into(),
        }
    }
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
    Address,
    Microsoft,
}

async fn check_target(
    client: &reqwest::Client,
    family: Family,
    url: &str,
    check: ResponseCheck,
) -> Result<(), String> {
    let response = client
        .get(url)
        .header("Cache-Control", "no-cache, no-store")
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                "连接超时".to_string()
            } else {
                let mut reason = error.without_url().to_string();
                // These failures alone do not establish that an IP family
                // is unavailable; provide an actionable probe-level message.
                if reason == "error sending request" {
                    reason = "连接失败（DNS、路由或 TLS）".to_string();
                }
                reason
            }
        })?;
    if matches!(check, ResponseCheck::NoContent) {
        return if response.status() == reqwest::StatusCode::NO_CONTENT {
            Ok(())
        } else {
            Err(format!("HTTP {}，响应不是 204", response.status().as_u16()))
        };
    }
    if response.status() != reqwest::StatusCode::OK {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "读取探测响应失败")? {
        if body.len() + chunk.len() > 4096 {
            return Err("探测响应过大".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    let body = String::from_utf8_lossy(&body);
    let valid = match check {
        ResponseCheck::Microsoft => body.trim() == "Microsoft Connect Test",
        ResponseCheck::Address => body
            .trim()
            .parse::<IpAddr>()
            .is_ok_and(|ip| family.matches(ip) && !ip.is_unspecified() && !ip.is_loopback()),
        ResponseCheck::NoContent => false,
    };
    if valid {
        Ok(())
    } else {
        Err("响应内容或地址族不符合预期".to_string())
    }
}

async fn probe_family(
    family: Family,
    addresses: Vec<String>,
    targets: &[(&str, ResponseCheck)],
) -> FamilyConnectivity {
    let started = std::time::Instant::now();
    // Public reachability follows the system/TUN route. Only constrain the
    // family, leaving source selection to the OS (including temporary IPv6).
    // HTTP proxies are excluded because they can conceal the real IP family.
    let builder = reqwest::Client::builder()
        .no_proxy()
        .local_address(family.source())
        .redirect(reqwest::redirect::Policy::none())
        .timeout(NETWORK_PROBE_TIMEOUT)
        .use_rustls_tls();
    let mut errors = Vec::new();
    if let Ok(client) = builder.build() {
        let mut probes: FuturesUnordered<_> = targets
            .iter()
            .map(|(url, check)| async {
                tokio::time::timeout(
                    NETWORK_PROBE_TIMEOUT,
                    check_target(&client, family, url, *check),
                )
                .await
                .unwrap_or_else(|_| Err("3 秒内无响应".to_string()))
            })
            .collect();
        while let Some(result) = probes.next().await {
            match result {
                Ok(()) => {
                    return FamilyConnectivity {
                        addresses,
                        status: "reachable".to_string(),
                        detail: "经系统路由完成对应地址族的外网响应校验".to_string(),
                        duration_ms: started.elapsed().as_millis(),
                    }
                }
                Err(error) => errors.push(error),
            }
        }
    } else {
        errors.push("无法创建探测连接".to_string());
    }
    let timed_out = errors
        .iter()
        .any(|error| error.contains("超时") || error.contains("无响应"));
    FamilyConnectivity {
        addresses,
        status: if timed_out { "timeout" } else { "unknown" }.to_string(),
        detail: format!(
            "外网探测{}，尚不能据此判定该地址族不可用。{}",
            if timed_out { "超时" } else { "未完成" },
            errors.join("；")
        ),
        duration_ms: started.elapsed().as_millis(),
    }
}

pub(crate) async fn probe(network: &serde_json::Value) -> DualStackReport {
    probe_with_updates(network, |_| {}).await
}

pub(crate) async fn probe_with_updates(
    network: &serde_json::Value,
    mut update: impl FnMut(&DualStackReport),
) -> DualStackReport {
    let interface = network["interfaceName"].as_str().unwrap_or("");
    let ip = network["ip"].as_str().unwrap_or("");
    let adapters = crate::network_inventory::adapters();
    let adapter = adapters.iter().find(|adapter| {
        adapter.interface_name == interface && adapter.ipv4.iter().any(|candidate| candidate == ip)
    });
    let ipv4 = adapter.map(|_| vec![ip.to_string()]).unwrap_or_default();
    let ipv6 = adapter
        .map(|adapter| adapter.ipv6.clone())
        .unwrap_or_default();
    let pending = |addresses| FamilyConnectivity {
        addresses,
        status: "checking".to_string(),
        detail: "正在探测".to_string(),
        duration_ms: 0,
    };
    let mut report = DualStackReport {
        interface_name: interface.to_string(),
        checked_at: chrono::Local::now().to_rfc3339(),
        scope: "system".to_string(),
        ipv4: pending(ipv4.clone()),
        ipv6: pending(ipv6.clone()),
    };
    let v4_targets = [
        (
            "https://cp.cloudflare.com/generate_204",
            ResponseCheck::NoContent,
        ),
        ("https://api.ipify.org", ResponseCheck::Address),
    ];
    let v6_targets = [
        ("https://api6.ipify.org", ResponseCheck::Address),
        (
            "http://ipv6.msftconnecttest.com/connecttest.txt",
            ResponseCheck::Microsoft,
        ),
    ];
    let mut probes = FuturesUnordered::new();
    probes.push(async { (false, probe_family(Family::V4, ipv4, &v4_targets).await) }.boxed());
    probes.push(async { (true, probe_family(Family::V6, ipv6, &v6_targets).await) }.boxed());
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
                    Family::V4,
                    vec!["192.0.2.99".to_string()],
                    &[(&target, ResponseCheck::NoContent)],
                ),
                probe_family(
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
            for expected in ["reachable", "unknown"] {
                let result = probe_family(
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
            let result = tokio::time::timeout(std::time::Duration::from_secs(2), probe_family(Family::V4, Vec::new(), &[(&slow_url, ResponseCheck::NoContent), (&fast_url, ResponseCheck::NoContent)])).await;
            slow_worker.abort();
            fast_worker.await.unwrap();
            assert_eq!(result.expect("a stalled target must not delay a valid response").status, "reachable");
        });
    }
}
