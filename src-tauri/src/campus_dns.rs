//! Campus DNS follows the physical network when known, with a service hint
//! otherwise. LGN wired uses its DHCP resolvers; Wi-Fi keeps its own resolver.

use crate::network_probe::NETWORK_PROBE_TIMEOUT;
use futures_util::{stream::FuturesUnordered, StreamExt};
use std::net::Ipv4Addr;
use std::time::Duration;

pub(crate) fn campus_dns_servers(
    host: &str,
    source_ipv4: Option<Ipv4Addr>,
) -> &'static [&'static str] {
    let wired = source_ipv4
        .map(|address| matches!(address.octets(), [172, 26, _, _]))
        .unwrap_or(host == super::LGN_HOST || host == super::LGN6_HOST);
    if wired {
        &["172.21.0.21:53", "172.21.201.22:53"]
    } else {
        &["10.21.200.28:53"]
    }
}

pub(crate) async fn query_campus_dns_ipv4(
    host: &str,
    source_ipv4: Option<Ipv4Addr>,
) -> Result<Vec<Ipv4Addr>, String> {
    query_ipv4_via_servers(
        host,
        source_ipv4,
        campus_dns_servers(host, source_ipv4),
        NETWORK_PROBE_TIMEOUT,
    )
    .await
}

async fn query_ipv4_via_servers(
    host: &str,
    source_ipv4: Option<Ipv4Addr>,
    servers: &[&str],
    timeout: Duration,
) -> Result<Vec<Ipv4Addr>, String> {
    let queries = async {
        let mut pending: FuturesUnordered<_> =
            servers
                .iter()
                .map(|server| async move {
                    (*server, query_dns_server(host, server, source_ipv4).await)
                })
                .collect();
        let mut errors = Vec::new();
        while let Some((server, result)) = pending.next().await {
            match result {
                Ok(addresses) => return Ok(addresses),
                Err(error) => errors.push(format!("{server}：{error}")),
            }
        }
        Err(errors.join("；"))
    };
    tokio::time::timeout(timeout, queries)
        .await
        .map_err(|_| format!("校园网 DNS（{}）查询超时", servers.join(" / ")))?
}

fn skip_dns_name(packet: &[u8], position: &mut usize) -> Result<(), String> {
    loop {
        let length = *packet.get(*position).ok_or("校园网 DNS 响应不完整")?;
        if length & 0xc0 == 0xc0 {
            if packet.get(*position + 1).is_none() {
                return Err("校园网 DNS 压缩指针不完整".to_string());
            }
            *position += 2;
            return Ok(());
        }
        *position += 1;
        if length == 0 {
            return Ok(());
        }
        *position = position
            .checked_add(length as usize)
            .filter(|next| *next <= packet.len())
            .ok_or("校园网 DNS 名称越界")?;
    }
}

async fn query_dns_server(
    host: &str,
    server: &str,
    source_ipv4: Option<std::net::Ipv4Addr>,
) -> Result<Vec<std::net::Ipv4Addr>, String> {
    let labels: Vec<&str> = host.split('.').collect();
    if labels.is_empty()
        || labels
            .iter()
            .any(|label| label.is_empty() || label.len() > 63)
    {
        return Err("校园网 DNS 查询域名无效".to_string());
    }
    let query_id = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
        & 0xffff) as u16;
    let mut query = Vec::with_capacity(64);
    query.extend_from_slice(&query_id.to_be_bytes());
    query.extend_from_slice(&[0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    for label in labels {
        query.push(label.len() as u8);
        query.extend_from_slice(label.as_bytes());
    }
    query.push(0);
    query.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);

    let bind_address =
        std::net::SocketAddrV4::new(source_ipv4.unwrap_or(std::net::Ipv4Addr::UNSPECIFIED), 0);
    let socket = tokio::net::UdpSocket::bind(bind_address)
        .await
        .map_err(|error| format!("无法创建校园网 DNS 查询：{error}"))?;
    socket
        .connect(server)
        .await
        .map_err(|error| format!("无法连接校园网 DNS：{error}"))?;
    socket
        .send(&query)
        .await
        .map_err(|error| format!("校园网 DNS 查询发送失败：{error}"))?;
    let mut packet = [0u8; 2048];
    let size = socket
        .recv(&mut packet)
        .await
        .map_err(|error| format!("校园网 DNS 查询失败：{error}"))?;
    let packet = &packet[..size];
    if packet.len() < 12 || u16::from_be_bytes([packet[0], packet[1]]) != query_id {
        return Err("校园网 DNS 返回了无效响应".to_string());
    }
    if packet[3] & 0x0f != 0 {
        return Err(format!("校园网 DNS 返回错误码 {}", packet[3] & 0x0f));
    }
    let question_count = u16::from_be_bytes([packet[4], packet[5]]) as usize;
    let answer_count = u16::from_be_bytes([packet[6], packet[7]]) as usize;
    let mut position = 12usize;
    for _ in 0..question_count {
        skip_dns_name(packet, &mut position)?;
        position = position
            .checked_add(4)
            .filter(|next| *next <= packet.len())
            .ok_or("校园网 DNS 问题段越界")?;
    }
    let mut addresses = Vec::new();
    for _ in 0..answer_count {
        skip_dns_name(packet, &mut position)?;
        if position + 10 > packet.len() {
            return Err("校园网 DNS 答案段不完整".to_string());
        }
        let record_type = u16::from_be_bytes([packet[position], packet[position + 1]]);
        let record_class = u16::from_be_bytes([packet[position + 2], packet[position + 3]]);
        let data_length = u16::from_be_bytes([packet[position + 8], packet[position + 9]]) as usize;
        position += 10;
        if position + data_length > packet.len() {
            return Err("校园网 DNS 记录数据越界".to_string());
        }
        if record_type == 1 && record_class == 1 && data_length == 4 {
            let address = std::net::Ipv4Addr::new(
                packet[position],
                packet[position + 1],
                packet[position + 2],
                packet[position + 3],
            );
            if !addresses.contains(&address) {
                addresses.push(address);
            }
        }
        position += data_length;
    }
    if addresses.is_empty() {
        Err(format!("校园网 DNS 未返回 {host} 的 IPv4 地址"))
    } else {
        Ok(addresses)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lgn_uses_the_wired_resolvers_without_changing_wifi_dns() {
        assert_eq!(
            campus_dns_servers("jfself.bjut.edu.cn", Some(Ipv4Addr::new(172, 26, 33, 10))),
            &["172.21.0.21:53", "172.21.201.22:53"]
        );
        assert_eq!(
            campus_dns_servers(super::super::LGN_HOST, Some(Ipv4Addr::new(10, 126, 80, 10))),
            &["10.21.200.28:53"]
        );
        assert_eq!(
            campus_dns_servers(super::super::LGN_HOST, None),
            &["172.21.0.21:53", "172.21.201.22:53"]
        );
        assert_eq!(
            campus_dns_servers(super::super::LGN6_HOST, None),
            &["172.21.0.21:53", "172.21.201.22:53"]
        );
        assert_eq!(
            campus_dns_servers(super::super::WLGN_HOST, None),
            &["10.21.200.28:53"]
        );
    }

    #[test]
    fn a_responsive_dns_server_does_not_wait_for_a_silent_primary() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let silent = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let responder = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let servers = [
                silent.local_addr().unwrap().to_string(),
                responder.local_addr().unwrap().to_string(),
            ];
            let server_names = servers.iter().map(String::as_str).collect::<Vec<_>>();
            let answer = async {
                let mut request = [0u8; 512];
                let (length, peer) = responder.recv_from(&mut request).await.unwrap();
                let mut response = request[..length].to_vec();
                response[2..4].copy_from_slice(&[0x81, 0x80]);
                response[6..8].copy_from_slice(&[0, 1]);
                response.extend_from_slice(&[
                    0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 172, 30, 201, 2,
                ]);
                responder.send_to(&response, peer).await.unwrap();
            };
            let lookup = query_ipv4_via_servers(
                super::super::LGN_HOST,
                Some(Ipv4Addr::LOCALHOST),
                &server_names,
                Duration::from_millis(500),
            );
            let (addresses, ()) = tokio::time::timeout(
                Duration::from_secs(2),
                futures_util::future::join(lookup, answer),
            )
            .await
            .unwrap();
            assert_eq!(addresses.unwrap(), vec![Ipv4Addr::new(172, 30, 201, 2)]);
            let mut query = [0u8; 512];
            assert!(
                silent.try_recv(&mut query).is_ok(),
                "both resolvers must be queried concurrently"
            );
        });
    }
}
