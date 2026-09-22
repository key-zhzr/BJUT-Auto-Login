//! Credential-free probes of both address families over the default route and
//! an explicitly selected physical link. Literal public destinations avoid
//! VPN Fake-IP DNS; TLS still verifies the public service's hostname.
use crate::{
    dual_stack::{DualStackReport, FamilyConnectivity},
    network_probe::NETWORK_PROBE_TIMEOUT,
};
use futures_util::{stream::FuturesUnordered, StreamExt};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Instant,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpSocket,
};

#[derive(Clone)]
pub(crate) struct Paths {
    pub(crate) system: DualStackReport,
    pub(crate) direct: DualStackReport,
}

impl Paths {
    pub(crate) fn step(&self) -> crate::DiagnosticStep {
        let mut lines = Vec::new();
        let mut pending = false;
        let mut online = false;
        let mut duration_ms = 0;
        for (path, report) in [
            ("当前网络（含 VPN）", &self.system),
            ("网卡直连（绕过 VPN）", &self.direct),
        ] {
            for (family, result) in [("IPv4", &report.ipv4), ("IPv6", &report.ipv6)] {
                pending |= result.status == "checking";
                online |= result.status == "reachable";
                duration_ms = duration_ms.max(result.duration_ms);
                let status = match result.status.as_str() {
                    "checking" => "检测中",
                    "reachable" => "通过",
                    "timeout" => "超时",
                    "not_configured" => "未配置",
                    "unavailable" => "无法探测",
                    _ => "未通过",
                };
                lines.push(format!(
                    "{path} · {family}：{status}（{} ms）{}",
                    result.duration_ms,
                    if matches!(
                        result.status.as_str(),
                        "checking" | "reachable" | "not_configured"
                    ) {
                        String::new()
                    } else {
                        format!(" · {}", result.detail)
                    }
                ));
            }
        }
        crate::DiagnosticStep {
            id: "internet".into(),
            label: "互联网连通性".into(),
            status: if pending {
                "checking"
            } else if online {
                "success"
            } else {
                "warning"
            }
            .into(),
            message: lines.join("\n"),
            duration_ms,
        }
    }
}

fn client_config() -> Arc<rustls::ClientConfig> {
    static TLS: std::sync::OnceLock<Arc<rustls::ClientConfig>> = std::sync::OnceLock::new();
    TLS.get_or_init(|| {
        Arc::new(
            rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .expect("supported TLS versions")
            .with_root_certificates(rustls::RootCertStore::from_iter(
                webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
            ))
            .with_no_client_auth(),
        )
    })
    .clone()
}

#[cfg(target_os = "windows")]
fn bind_interface(socket: &TcpSocket, interface: &str, ipv6: bool) -> Result<(), String> {
    use std::os::windows::io::AsRawSocket;
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::NO_ERROR,
            NetworkManagement::{
                IpHelper::{ConvertInterfaceAliasToLuid, ConvertInterfaceLuidToIndex},
                Ndis::NET_LUID_LH,
            },
            Networking::WinSock::{
                setsockopt, IPPROTO_IP, IPPROTO_IPV6, IPV6_UNICAST_IF, IP_UNICAST_IF, SOCKET,
            },
        },
    };
    let name: Vec<u16> = interface.encode_utf16().chain(Some(0)).collect();
    let mut luid = NET_LUID_LH::default();
    let mut index = 0;
    // SAFETY: NUL-terminated name and output records live for both calls.
    if unsafe { ConvertInterfaceAliasToLuid(PCWSTR(name.as_ptr()), &mut luid) } != NO_ERROR
        || unsafe { ConvertInterfaceLuidToIndex(&luid, &mut index) } != NO_ERROR
        || index == 0
    {
        return Err("网卡已变化，请重新诊断".into());
    }
    // Windows requires network byte order for IP_UNICAST_IF, host order for IPv6.
    let value = if ipv6 { index } else { index.to_be() }.to_ne_bytes();
    let (level, option) = if ipv6 {
        (IPPROTO_IPV6.0, IPV6_UNICAST_IF as i32)
    } else {
        (IPPROTO_IP.0, IP_UNICAST_IF as i32)
    };
    // SAFETY: socket is open; setsockopt copies the four-byte index synchronously.
    if unsafe {
        setsockopt(
            SOCKET(socket.as_raw_socket() as usize),
            level,
            option,
            Some(&value),
        )
    } != 0
    {
        return Err("无法绑定所选网卡".into());
    }
    Ok(())
}
#[cfg(target_os = "macos")]
fn bind_interface(socket: &TcpSocket, interface: &str, ipv6: bool) -> Result<(), String> {
    let name = std::ffi::CString::new(interface).map_err(|_| "网卡名称无效")?;
    // SAFETY: name is a valid NUL-terminated C string.
    let index = std::num::NonZeroU32::new(unsafe { libc::if_nametoindex(name.as_ptr()) })
        .ok_or("网卡已断开")?;
    let socket = socket2::SockRef::from(socket);
    if ipv6 {
        socket.bind_device_by_index_v6(Some(index))
    } else {
        socket.bind_device_by_index_v4(Some(index))
    }
    .map_err(|_| "无法绑定所选网卡".into())
}
#[cfg(target_os = "linux")]
fn bind_interface(socket: &TcpSocket, interface: &str, _: bool) -> Result<(), String> {
    socket2::SockRef::from(socket)
        .bind_device(Some(interface.as_bytes()))
        .map_err(|_| "系统未允许网卡直连探测".into())
}
#[cfg(not(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "macos",
    target_os = "android"
)))]
fn bind_interface(_: &TcpSocket, _: &str, _: bool) -> Result<(), String> {
    Err("此平台暂不支持网卡直连探测".into())
}

#[cfg(target_os = "android")]
fn bind_android(
    socket: &TcpSocket,
    network: &serde_json::Value,
    direct: bool,
) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    #[link(name = "android")]
    extern "C" {
        fn android_setsocknetwork(network: u64, fd: i32) -> i32;
    }
    let key = if direct {
        "physicalNetworkHandle"
    } else {
        "defaultNetworkHandle"
    };
    let handle = network[key]
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value != 0)
        .ok_or("未取得当前网络")?;
    // SAFETY: Network.getNetworkHandle() is captured from the selected Network;
    // socket is alive. Binding is per socket and never changes the process route.
    if unsafe { android_setsocknetwork(handle, socket.as_raw_fd()) } != 0 {
        return Err("系统不允许使用此网络（请检查 VPN 的直连策略）".into());
    }
    Ok(())
}

async fn target(
    network: &serde_json::Value,
    source: Option<IpAddr>,
    direct: bool,
    destination: IpAddr,
) -> Result<(), (&'static str, String)> {
    let ipv6 = destination.is_ipv6();
    let socket = if ipv6 {
        TcpSocket::new_v6()
    } else {
        TcpSocket::new_v4()
    }
    .map_err(|_| ("connection_error", "无法创建连接".into()))?;
    #[cfg(target_os = "android")]
    bind_android(&socket, network, direct).map_err(|detail| ("unavailable", detail))?;
    #[cfg(not(target_os = "android"))]
    if direct {
        bind_interface(
            &socket,
            network["interfaceName"].as_str().unwrap_or(""),
            ipv6,
        )
        .map_err(|detail| ("unavailable", detail))?;
    }
    if let Some(source) = source {
        socket
            .bind(SocketAddr::new(source, 0))
            .map_err(|_| ("unavailable", "网卡地址已变化".into()))?;
    }
    let stream = socket
        .connect(SocketAddr::new(destination, 443))
        .await
        .map_err(|_| ("connection_error", "目标连接未建立".into()))?;
    let server =
        rustls::pki_types::ServerName::try_from("cloudflare-dns.com").expect("literal host");
    let mut tls = tokio_rustls::TlsConnector::from(client_config())
        .connect(server, stream)
        .await
        .map_err(|_| ("tls_error", "HTTPS 校验未通过".into()))?;
    tls.write_all(b"GET /dns-query?name=example.com&type=A HTTP/1.1\r\nHost: cloudflare-dns.com\r\nAccept: application/dns-json\r\nConnection: close\r\nCache-Control: no-cache\r\n\r\n").await
        .map_err(|_| ("connection_error", "请求发送失败".into()))?;
    let mut response = Vec::new();
    loop {
        let mut buffer = [0u8; 1024];
        let count = tls
            .read(&mut buffer)
            .await
            .map_err(|_| ("connection_error", "响应读取失败".into()))?;
        if count == 0 {
            return Err(("response_error", "未收到完整响应".into()));
        }
        response.extend_from_slice(&buffer[..count]);
        if response.windows(4).any(|value| value == b"\r\n\r\n") {
            return if valid_response(&response) {
                Ok(())
            } else {
                Err(("response_error", "外网响应未通过校验".into()))
            };
        }
        if response.len() > 8192 {
            return Err(("response_error", "响应超过大小限制".into()));
        }
    }
}
fn valid_response(response: &[u8]) -> bool {
    response.starts_with(b"HTTP/1.1 200 ") || response.starts_with(b"HTTP/1.0 200 ")
}
async fn family(
    network: &serde_json::Value,
    direct: bool,
    ipv6: bool,
    addresses: Vec<String>,
) -> FamilyConnectivity {
    let start = Instant::now();
    if direct && addresses.is_empty() {
        return FamilyConnectivity {
            addresses,
            status: "not_configured".into(),
            detail: "此网卡未配置可用地址".into(),
            duration_ms: 0,
        };
    }
    let source = direct
        .then(|| {
            addresses
                .first()
                .and_then(|address| address.parse::<IpAddr>().ok())
        })
        .flatten();
    let destinations = if ipv6 {
        ["2606:4700:4700::1111", "2606:4700:4700::1001"]
    } else {
        ["1.1.1.1", "1.0.0.1"]
    };
    let work = async {
        let mut jobs: FuturesUnordered<_> = destinations
            .iter()
            .map(|address| {
                target(
                    network,
                    source,
                    direct,
                    address.parse().expect("literal IP"),
                )
            })
            .collect();
        let mut failure = ("connection_error", "未取得探测结果".to_string());
        while let Some(result) = jobs.next().await {
            match result {
                Ok(()) => return Ok(()),
                Err(error) => failure = error,
            }
        }
        Err(failure)
    };
    let result = tokio::time::timeout(NETWORK_PROBE_TIMEOUT, work)
        .await
        .unwrap_or_else(|_| Err(("timeout", "3 秒内未收到响应".into())));
    let (status, detail) = match result {
        Ok(()) => ("reachable", "HTTPS 外网连接已验证".into()),
        Err(error) => error,
    };
    FamilyConnectivity {
        addresses,
        status: status.into(),
        detail,
        duration_ms: start.elapsed().as_millis(),
    }
}

pub(crate) async fn probe(network: &serde_json::Value, mut update: impl FnMut(&Paths)) -> Paths {
    let inventory = crate::network_inventory::adapters();
    let adapter = inventory.iter().find(|adapter| {
        adapter.selectable
            && adapter.connected
            && adapter.interface_name == network["interfaceName"].as_str().unwrap_or("")
            && adapter
                .ipv4
                .iter()
                .any(|ip| Some(ip.as_str()) == network["ip"].as_str())
    });
    let v4 = adapter
        .map(|_| vec![network["ip"].as_str().unwrap_or("").to_string()])
        .unwrap_or_default();
    let v6 = adapter
        .map(|adapter| adapter.ipv6.clone())
        .unwrap_or_default();
    let mut paths = Paths {
        system: crate::dual_stack::unavailable("正在检测"),
        direct: crate::dual_stack::unavailable("正在检测"),
    };
    paths.direct.scope = "physical".into();
    paths.direct.interface_name = network["interfaceName"].as_str().unwrap_or("").into();
    for report in [&mut paths.system, &mut paths.direct] {
        report.ipv4.status = "checking".into();
        report.ipv6.status = "checking".into();
    }
    update(&paths);
    let physical_available = adapter.is_some();
    let mut jobs: FuturesUnordered<_> = [
        (false, false, Vec::new()),
        (false, true, Vec::new()),
        (true, false, v4),
        (true, true, v6),
    ]
    .into_iter()
    .map(|(direct, ipv6, addresses)| async move {
        let result = if direct && !physical_available {
            FamilyConnectivity {
                addresses,
                status: "unavailable".into(),
                detail: "未取得所选网卡，请刷新后重试".into(),
                duration_ms: 0,
            }
        } else {
            family(network, direct, ipv6, addresses).await
        };
        (direct, ipv6, result)
    })
    .collect();
    while let Some((direct, ipv6, result)) = jobs.next().await {
        let report = if direct {
            &mut paths.direct
        } else {
            &mut paths.system
        };
        if ipv6 {
            report.ipv6 = result;
        } else {
            report.ipv4 = result;
        }
        update(&paths);
    }
    paths
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires access to the public diagnostic service"]
    fn public_ipv4_service_accepts_the_probe() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let report = super::family(&serde_json::json!({}), false, false, Vec::new()).await;
            assert_eq!(report.status, "reachable", "{}", report.detail);
        });
    }

    #[test]
    fn redirects_captive_pages_and_truncated_status_are_not_success() {
        assert!(super::valid_response(b"HTTP/1.1 200 OK\r\n\r\n"));
        for value in [
            b"HTTP/1.1 302 Found".as_slice(),
            b"HTTP/1.1 2000 NO",
            b"HTTP/1.1 20",
        ] {
            assert!(!super::valid_response(value));
        }
    }
}
