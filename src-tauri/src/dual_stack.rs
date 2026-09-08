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

#[derive(Debug)]
struct ProbeFailure {
    kind: &'static str,
    detail: String,
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
fn request_failure(error: reqwest::Error) -> ProbeFailure {
    use std::error::Error;
    if error.is_timeout() {
        return ProbeFailure::timeout();
    }
    let mut causes = String::new();
    let mut source = error.source();
    while let Some(cause) = source {
        causes.push_str(&cause.to_string().to_ascii_lowercase());
        source = cause.source();
    }
    let (kind, detail) = if ["dns", "resolve", "lookup", "getaddrinfo", "no such host"]
        .iter()
        .any(|word| causes.contains(word))
    {
        (
            "dns_error",
            "DNS 未能解析探测目标；请核对系统或 VPN 的 DNS 配置",
        )
    } else if ["tls", "ssl", "certificate", "cert verify"]
        .iter()
        .any(|word| causes.contains(word))
    {
        (
            "tls_error",
            "TLS 校验或握手未通过；请核对系统时间、证书或网络拦截",
        )
    } else {
        (
            "connection_error",
            "连接未建立或中断；请核对路由、VPN 与认证状态",
        )
    };
    // Only classification leaves this function; raw source errors may contain URLs.
    ProbeFailure {
        kind,
        detail: detail.into(),
    }
}

async fn check_target(
    client: &reqwest::Client,
    family: Family,
    url: &str,
    check: ResponseCheck,
) -> Result<(), ProbeFailure> {
    let response = client
        .get(url)
        .header("Cache-Control", "no-cache, no-store")
        .send()
        .await
        .map_err(request_failure)?;
    if matches!(check, ResponseCheck::NoContent) {
        return if response.status() == reqwest::StatusCode::NO_CONTENT {
            Ok(())
        } else {
            Err(ProbeFailure::response(format!(
                "HTTP {}，响应不是 204（可能被认证页拦截）",
                response.status().as_u16()
            )))
        };
    }
    if response.status() != reqwest::StatusCode::OK {
        return Err(ProbeFailure::response(format!(
            "HTTP {}",
            response.status().as_u16()
        )));
    }
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(request_failure)? {
        if body.len() + chunk.len() > 4096 {
            return Err(ProbeFailure::response("探测响应过大"));
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
        Err(ProbeFailure::response(
            "响应内容或地址族不符合预期（可能被认证页拦截）",
        ))
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
                .unwrap_or_else(|_| Err(ProbeFailure::timeout()))
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
        errors.push(ProbeFailure {
            kind: "connection_error",
            detail: "无法创建探测连接".into(),
        });
    }
    let status = if errors.iter().any(|error| error.kind == "timeout") {
        "timeout"
    } else if errors.iter().all(|error| error.kind == errors[0].kind) {
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
        generation: 0,
        probe_id: 0,
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
    let missing_v4 = family_unconfigured(&adapters, false);
    let missing_v6 = family_unconfigured(&adapters, true);
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
        probes.push(async { (false, probe_family(Family::V4, ipv4, &v4_targets).await) }.boxed());
    }
    if missing_v6 {
        report.ipv6 = absent(ipv6);
    } else {
        probes.push(async { (true, probe_family(Family::V6, ipv6, &v6_targets).await) }.boxed());
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
            for expected in ["reachable", "response_error"] {
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
