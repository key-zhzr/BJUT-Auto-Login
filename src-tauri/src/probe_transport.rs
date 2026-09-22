//! Socket-level, credential-free connectivity probes. HTTP proxies cannot
//! conceal the address family; direct DNS and TCP use the same physical link.
use crate::dual_stack::ProbeFailure;
use futures_util::{stream::FuturesUnordered, StreamExt};
use http_body_util::BodyExt;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpSocket,
};

fn failure(kind: &'static str, detail: &str) -> ProbeFailure {
    ProbeFailure {
        kind,
        detail: detail.into(),
    }
}

fn bind_route(
    socket: &socket2::SockRef<'_>,
    network: &serde_json::Value,
    direct: bool,
    _ipv6: bool,
) -> Result<(), ProbeFailure> {
    #[cfg(target_os = "android")]
    bind_android(socket, network, direct).map_err(|detail| ProbeFailure {
        kind: "unavailable",
        detail,
    })?;
    #[cfg(not(target_os = "android"))]
    if direct {
        bind_interface(
            socket,
            network["interfaceName"].as_str().unwrap_or(""),
            _ipv6,
        )
        .map_err(|detail| ProbeFailure {
            kind: "unavailable",
            detail,
        })?;
    }
    Ok(())
}

async fn exchange(
    stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
    url: &reqwest::Url,
) -> Result<(u16, Vec<u8>), ProbeFailure> {
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream))
            .await
            .map_err(|_| failure("connection_error", "HTTP 连接未建立"))?;
    let path = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_string(),
    };
    let request = hyper::Request::builder()
        .uri(path)
        .header(
            "Host",
            match url.port() {
                Some(port) => format!("{}:{port}", url.host_str().unwrap_or("")),
                None => url.host_str().unwrap_or("").to_string(),
            },
        )
        .header("Cache-Control", "no-cache, no-store")
        .header("Connection", "close")
        .body(http_body_util::Empty::<hyper::body::Bytes>::new())
        .map_err(|_| failure("response_error", "探测目标无效"))?;
    let response = async {
        let mut response = sender
            .send_request(request)
            .await
            .map_err(|_| failure("connection_error", "未收到 HTTP 响应"))?;
        let status = response.status().as_u16();
        if status == 204 {
            return Ok((status, Vec::new()));
        }
        let mut bytes = Vec::new();
        while let Some(frame) = response.body_mut().frame().await {
            let frame = frame.map_err(|_| failure("response_error", "响应未读取完整"))?;
            if let Some(data) = frame.data_ref() {
                if bytes.len() + data.len() > 4096 {
                    return Err(failure("response_error", "探测响应过大"));
                }
                bytes.extend_from_slice(data);
            }
        }
        Ok((status, bytes))
    };
    // Drive HTTP without detached tasks: cancelling a losing target or a run
    // drops its connection/TLS/body work immediately. Platform DNS calls may
    // finish in their resolver worker, but cannot publish a late result.
    tokio::pin!(response, connection);
    match futures_util::future::select(response, connection).await {
        futures_util::future::Either::Left((value, _)) => value,
        futures_util::future::Either::Right((_, response)) => response.await,
    }
}

async fn at_address(
    network: &serde_json::Value,
    direct: bool,
    url: &reqwest::Url,
    address: IpAddr,
) -> Result<(u16, Vec<u8>), ProbeFailure> {
    let socket = if address.is_ipv6() {
        TcpSocket::new_v6()
    } else {
        TcpSocket::new_v4()
    }
    .map_err(|_| failure("connection_error", "无法创建连接"))?;
    bind_route(
        &socket2::SockRef::from(&socket),
        network,
        direct,
        address.is_ipv6(),
    )?;
    let stream = socket
        .connect(SocketAddr::new(
            address,
            url.port_or_known_default().unwrap_or(80),
        ))
        .await
        .map_err(|_| failure("connection_error", "目标连接未建立"))?;
    if url.scheme() == "https" {
        let name =
            rustls::pki_types::ServerName::try_from(url.host_str().unwrap_or("").to_string())
                .map_err(|_| failure("tls_error", "HTTPS 目标无效"))?;
        let tls = tokio_rustls::TlsConnector::from(client_config())
            .connect(name, stream)
            .await
            .map_err(|_| failure("tls_error", "HTTPS 证书校验或握手未通过"))?;
        exchange(tls, url).await
    } else {
        exchange(stream, url).await
    }
}

pub(crate) async fn request(
    network: &serde_json::Value,
    direct: bool,
    ipv6: bool,
    url: &str,
) -> Result<(u16, Vec<u8>), ProbeFailure> {
    let url = reqwest::Url::parse(url).map_err(|_| failure("response_error", "探测目标无效"))?;
    let host = url.host_str().unwrap_or("").trim_matches(['[', ']']);
    let addresses = if let Ok(ip) = host.parse::<IpAddr>() {
        vec![ip]
    } else {
        resolve(network, direct, ipv6, host).await?
    };
    let mut jobs: FuturesUnordered<_> = addresses
        .into_iter()
        .filter(|ip| ip.is_ipv6() == ipv6)
        .take(4)
        .map(|ip| at_address(network, direct, &url, ip))
        .collect();
    let mut error = failure("dns_error", "DNS 未返回此地址族的目标地址");
    while let Some(result) = jobs.next().await {
        match result {
            Ok(response) => return Ok(response),
            Err(value) => error = value,
        }
    }
    Err(error)
}

#[cfg(target_os = "android")]
async fn resolve(
    network: &serde_json::Value,
    direct: bool,
    ipv6: bool,
    host: &str,
) -> Result<Vec<IpAddr>, ProbeFailure> {
    let key = if direct {
        "physicalNetworkHandle"
    } else {
        "defaultNetworkHandle"
    };
    let handle = network[key]
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|handle| *handle != 0)
        .ok_or_else(|| failure("unavailable", "未取得当前网络"))?;
    let host = std::ffi::CString::new(host).map_err(|_| failure("dns_error", "DNS 目标无效"))?;
    // DNS must use the same Android Network as TCP. The process may temporarily
    // be bound to campus Wi-Fi by an authentication operation.
    tokio::task::spawn_blocking(move || {
        #[link(name = "android")]
        extern "C" {
            fn android_getaddrinfofornetwork(
                network: u64,
                node: *const libc::c_char,
                service: *const libc::c_char,
                hints: *const libc::addrinfo,
                result: *mut *mut libc::addrinfo,
            ) -> libc::c_int;
        }
        let mut result = std::ptr::null_mut();
        // SAFETY: addrinfo accepts zero-initialized optional fields; pointers
        // remain valid for the call. Only matching sockaddr records are read.
        let mut hints: libc::addrinfo = unsafe { std::mem::zeroed() };
        hints.ai_family = if ipv6 { libc::AF_INET6 } else { libc::AF_INET };
        hints.ai_socktype = libc::SOCK_STREAM;
        if unsafe {
            android_getaddrinfofornetwork(
                handle,
                host.as_ptr(),
                std::ptr::null(),
                &hints,
                &mut result,
            )
        } != 0
        {
            return Err(failure("dns_error", "此网络 DNS 未能解析探测目标"));
        }
        let mut addresses = Vec::new();
        let mut current = result;
        while !current.is_null() {
            // SAFETY: traversal and freeing follow getaddrinfo's ownership contract.
            let entry = unsafe { &*current };
            if !entry.ai_addr.is_null() {
                if entry.ai_family == libc::AF_INET
                    && entry.ai_addrlen as usize >= std::mem::size_of::<libc::sockaddr_in>()
                {
                    let address = unsafe { &*entry.ai_addr.cast::<libc::sockaddr_in>() };
                    addresses.push(IpAddr::from(address.sin_addr.s_addr.to_ne_bytes()));
                } else if entry.ai_family == libc::AF_INET6
                    && entry.ai_addrlen as usize >= std::mem::size_of::<libc::sockaddr_in6>()
                {
                    let address = unsafe { &*entry.ai_addr.cast::<libc::sockaddr_in6>() };
                    addresses.push(IpAddr::from(address.sin6_addr.s6_addr));
                }
            }
            current = entry.ai_next;
        }
        unsafe { libc::freeaddrinfo(result) };
        Ok(addresses)
    })
    .await
    .map_err(|_| failure("dns_error", "DNS 查询未完成"))?
}

#[cfg(not(target_os = "android"))]
async fn resolve(
    network: &serde_json::Value,
    direct: bool,
    ipv6: bool,
    host: &str,
) -> Result<Vec<IpAddr>, ProbeFailure> {
    if !direct {
        return tokio::net::lookup_host((host, 0))
            .await
            .map(|values| {
                values
                    .map(|address| address.ip())
                    .filter(|ip| ip.is_ipv6() == ipv6)
                    .collect()
            })
            .map_err(|_| failure("dns_error", "系统 DNS 未能解析探测目标"));
    }
    // Never reuse system Fake-IP results for a socket bypassing the TUN.
    // Configured campus resolvers and public fallback DNS all use the selected
    // interface. Failure to bind is terminal; there is no unbound retry.
    let mut servers: Vec<IpAddr> = network["lgnLinkConfiguration"]["dnsServers"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(network["dnsServers"].as_array().into_iter().flatten())
        .filter_map(|value| value.as_str()?.parse().ok())
        .collect();
    servers.extend(
        ["223.5.5.5", "223.6.6.6", "2400:3200::1"]
            .iter()
            .filter_map(|value| value.parse::<IpAddr>().ok()),
    );
    servers.sort();
    servers.dedup();
    let mut jobs: FuturesUnordered<_> = servers
        .into_iter()
        .map(|server| dns_query(network, host, ipv6, server))
        .collect();
    let mut last = failure("dns_error", "直连 DNS 未能解析探测目标");
    while let Some(value) = jobs.next().await {
        match value {
            Ok(addresses) if !addresses.is_empty() => return Ok(addresses),
            Err(error) => last = error,
            _ => {}
        }
    }
    Err(last)
}

#[cfg(not(target_os = "android"))]
async fn dns_query(
    network: &serde_json::Value,
    host: &str,
    ipv6: bool,
    server: IpAddr,
) -> Result<Vec<IpAddr>, ProbeFailure> {
    static NEXT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(1);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut query = id.to_be_bytes().to_vec();
    query.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in host.split('.') {
        query.push(label.len() as u8);
        query.extend_from_slice(label.as_bytes());
    }
    query.extend_from_slice(&[0, 0, if ipv6 { 28 } else { 1 }, 0, 1]);
    let socket = tokio::net::UdpSocket::bind(if server.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    })
    .await
    .map_err(|_| failure("dns_error", "无法创建 DNS 查询"))?;
    bind_route(
        &socket2::SockRef::from(&socket),
        network,
        true,
        server.is_ipv6(),
    )?;
    socket
        .connect(SocketAddr::new(server, 53))
        .await
        .map_err(|_| failure("dns_error", "无法连接直连 DNS"))?;
    socket
        .send(&query)
        .await
        .map_err(|_| failure("dns_error", "DNS 查询发送失败"))?;
    let mut bytes = [0; 4096];
    let count = socket
        .recv(&mut bytes)
        .await
        .map_err(|_| failure("dns_error", "DNS 响应读取失败"))?;
    parse_dns(&bytes[..count], &query, ipv6)
        .ok_or_else(|| failure("dns_error", "DNS 未返回有效目标地址"))
}

#[cfg(any(test, not(target_os = "android")))]
fn parse_dns(packet: &[u8], query: &[u8], ipv6: bool) -> Option<Vec<IpAddr>> {
    // Match ID and the entire question; reject truncated/failed replies. Only
    // answer records of the requested family are allowed through.
    if packet.len() < query.len()
        || packet[..2] != query[..2]
        || packet[2] & 0xfa != 0x80
        || packet[3] & 0x0f != 0
        || packet[4..6] != [0, 1]
        || packet[12..query.len()] != query[12..]
    {
        return None;
    }
    fn name(packet: &[u8], pos: &mut usize) -> Option<()> {
        loop {
            let size = *packet.get(*pos)?;
            *pos += 1;
            if size == 0 {
                return Some(());
            }
            if size & 0xc0 == 0xc0 {
                packet.get(*pos)?;
                *pos += 1;
                return Some(());
            }
            if size > 63 {
                return None;
            }
            *pos = pos.checked_add(usize::from(size))?;
            if *pos > packet.len() {
                return None;
            }
        }
    }
    let mut pos = query.len();
    let mut result = Vec::new();
    for _ in 0..u16::from_be_bytes([packet[6], packet[7]]) {
        name(packet, &mut pos)?;
        let header = packet.get(pos..pos.checked_add(10)?)?;
        pos += 10;
        let length = usize::from(u16::from_be_bytes([header[8], header[9]]));
        let body = packet.get(pos..pos.checked_add(length)?)?;
        pos += length;
        let address = match (u16::from_be_bytes([header[0], header[1]]), length, ipv6) {
            (1, 4, false) => Some(IpAddr::from(<[u8; 4]>::try_from(body).ok()?)),
            (28, 16, true) => Some(IpAddr::from(<[u8; 16]>::try_from(body).ok()?)),
            _ => None,
        };
        if header[2..4] == [0, 1] {
            if let Some(ip) = address.filter(|ip| !ip.is_unspecified() && !ip.is_loopback() && !ip.is_multicast() && !matches!(ip, IpAddr::V4(v4) if v4.octets()[0] == 198 && (v4.octets()[1] == 18 || v4.octets()[1] == 19))) {
                if !result.contains(&ip) { result.push(ip); }
            }
        }
    }
    (!result.is_empty()).then_some(result)
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
fn bind_interface(
    socket: &socket2::SockRef<'_>,
    interface: &str,
    ipv6: bool,
) -> Result<(), String> {
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
fn bind_interface(
    socket: &socket2::SockRef<'_>,
    interface: &str,
    ipv6: bool,
) -> Result<(), String> {
    let name = std::ffi::CString::new(interface).map_err(|_| "网卡名称无效")?;
    // SAFETY: name is a valid NUL-terminated C string.
    let index = std::num::NonZeroU32::new(unsafe { libc::if_nametoindex(name.as_ptr()) })
        .ok_or("网卡已断开")?;
    if ipv6 {
        socket.bind_device_by_index_v6(Some(index))
    } else {
        socket.bind_device_by_index_v4(Some(index))
    }
    .map_err(|_| "无法绑定所选网卡".into())
}
#[cfg(target_os = "linux")]
fn bind_interface(socket: &socket2::SockRef<'_>, interface: &str, _: bool) -> Result<(), String> {
    socket
        .bind_device(Some(interface.as_bytes()))
        .map_err(|_| "系统未允许网卡直连探测".into())
}
#[cfg(not(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "macos",
    target_os = "android"
)))]
fn bind_interface(_: &socket2::SockRef<'_>, _: &str, _: bool) -> Result<(), String> {
    Err("此平台暂不支持网卡直连探测".into())
}

#[cfg(target_os = "android")]
fn bind_android(
    socket: &socket2::SockRef<'_>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_reply_checks_question_family_and_rejects_fake_ip() {
        let query = [
            0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, b'a', 0, 0, 1, 0, 1,
        ];
        let mut packet = query.to_vec();
        packet[2] = 0x81;
        packet[3] = 0x80;
        packet[7] = 1;
        packet.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 203, 0, 113, 10]);
        assert_eq!(
            parse_dns(&packet, &query, false).unwrap(),
            vec!["203.0.113.10".parse::<IpAddr>().unwrap()]
        );
        assert!(parse_dns(&packet, &query, true).is_none());
        let mut bad = packet.clone();
        bad[0] = 0;
        assert!(parse_dns(&bad, &query, false).is_none());
        let mut bad = packet.clone();
        bad[13] = b'b';
        assert!(parse_dns(&bad, &query, false).is_none());
        let mut bad = packet.clone();
        bad[2] |= 2;
        assert!(parse_dns(&bad, &query, false).is_none());
        let mut bad = packet.clone();
        let len = bad.len();
        bad[len - 4..].copy_from_slice(&[198, 18, 0, 1]);
        assert!(parse_dns(&bad, &query, false).is_none());
        for length in 0..packet.len() {
            assert!(parse_dns(&packet[..length], &query, false).is_none());
        }
        let mut query6 = query;
        query6[16] = 28;
        let mut reply6 = query6.to_vec();
        reply6[2] = 0x81;
        reply6[3] = 0x80;
        reply6[7] = 1;
        reply6.extend_from_slice(&[0xc0, 12, 0, 28, 0, 1, 0, 0, 0, 60, 0, 16]);
        let ip: std::net::Ipv6Addr = "2001:db8::1234".parse().unwrap();
        reply6.extend_from_slice(&ip.octets());
        assert_eq!(
            parse_dns(&reply6, &query6, true).unwrap(),
            vec![IpAddr::V6(ip)]
        );
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn missing_interface_never_falls_back_to_system_route() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/", server.local_addr().unwrap());
            let error = request(
                &serde_json::json!({"interfaceName":"bjut-missing"}),
                true,
                false,
                &url,
            )
            .await
            .unwrap_err();
            assert_eq!(error.kind, "unavailable");
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(20), server.accept())
                    .await
                    .is_err()
            );
        });
    }
}
