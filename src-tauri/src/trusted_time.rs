//! Read network time for an advisory recharge-hours indicator. Never set the
//! device clock, and never use this result to authorize or deny a payment.
use aes_gcm::aead::{rand_core::RngCore, OsRng};
use futures_util::{stream::FuturesUnordered, StreamExt};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const SERVERS: &[&str] = &["time.cloudflare.com", "ntp.aliyun.com"];
const NTP_UNIX_OFFSET: u64 = 2_208_988_800;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NetworkTime {
    unix_ms: u64,
    source: String,
}

struct Sample {
    time: NetworkTime,
    sampled: Instant,
}
static CACHE: OnceLock<tokio::sync::Mutex<Option<Sample>>> = OnceLock::new();

fn parse_response(packet: &[u8], origin: &[u8; 8]) -> Result<u64, String> {
    if packet.len() < 48
        || packet[0] & 7 != 4
        || !matches!((packet[0] >> 3) & 7, 3 | 4)
        || packet[0] >> 6 == 3
        || !(1..=15).contains(&packet[1])
        || &packet[24..32] != origin
    {
        return Err("时间服务器响应无效".into());
    }
    let seconds = u32::from_be_bytes(packet[40..44].try_into().unwrap()) as u64;
    let fraction = u32::from_be_bytes(packet[44..48].try_into().unwrap()) as u64;
    if seconds == 0 && fraction == 0 {
        return Err("时间服务器未返回有效时间".into());
    }
    // NTP's seconds wrap in 2036. This application accepts the current era and
    // the next one without depending on a possibly incorrect device clock.
    let seconds = if seconds < NTP_UNIX_OFFSET {
        seconds + (1u64 << 32)
    } else {
        seconds
    };
    let unix_ms = (seconds - NTP_UNIX_OFFSET) * 1000 + ((fraction * 1000) >> 32);
    if !(946_684_800_000..4_102_444_800_000).contains(&unix_ms) {
        return Err("网络时间超出有效范围".into());
    }
    Ok(unix_ms)
}

async fn query(server: &str) -> Result<NetworkTime, String> {
    let addresses = tokio::net::lookup_host((server, 123))
        .await
        .map_err(|_| "无法解析时间服务器")?;
    let address = addresses
        .into_iter()
        .find(|address| address.is_ipv4())
        .ok_or("时间服务器没有可用地址")?;
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|_| "无法创建校时连接")?;
    socket
        .connect(address)
        .await
        .map_err(|_| "无法连接时间服务器")?;
    let mut request = [0u8; 48];
    request[0] = 0x23;
    let mut nonce = [0u8; 8];
    OsRng.fill_bytes(&mut nonce);
    request[40..48].copy_from_slice(&nonce);
    let started = Instant::now();
    socket
        .send(&request)
        .await
        .map_err(|_| "校时请求未能发送")?;
    let mut response = [0u8; 512];
    let length = socket
        .recv(&mut response)
        .await
        .map_err(|_| "未收到校时响应")?;
    let unix_ms =
        parse_response(&response[..length], &nonce)? + started.elapsed().as_millis() as u64 / 2;
    Ok(NetworkTime {
        unix_ms,
        source: server.into(),
    })
}

#[tauri::command]
pub(crate) async fn get_network_time() -> Result<NetworkTime, String> {
    let mut cache = CACHE
        .get_or_init(|| tokio::sync::Mutex::new(None))
        .lock()
        .await;
    if let Some(sample) = cache
        .as_ref()
        .filter(|sample| sample.sampled.elapsed() < Duration::from_secs(300))
    {
        return Ok(NetworkTime {
            unix_ms: sample.time.unix_ms + sample.sampled.elapsed().as_millis() as u64,
            source: sample.time.source.clone(),
        });
    }
    let queries = async {
        let mut pending: FuturesUnordered<_> = SERVERS.iter().map(|server| query(server)).collect();
        while let Some(result) = pending.next().await {
            if let Ok(time) = result {
                return Ok(time);
            }
        }
        Err("暂时无法获取网络时间".to_string())
    };
    let time = tokio::time::timeout(Duration::from_secs(3), queries)
        .await
        .map_err(|_| "网络校时超时".to_string())??;
    *cache = Some(Sample {
        time: time.clone(),
        sampled: Instant::now(),
    });
    Ok(time)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_origin_sync_status_and_ntp_era() {
        let origin = [7u8; 8];
        let mut packet = [0u8; 48];
        packet[0] = 0x24;
        packet[1] = 2;
        packet[24..32].copy_from_slice(&origin);
        let unix = 1_789_977_600u64;
        packet[40..44].copy_from_slice(&((unix + NTP_UNIX_OFFSET) as u32).to_be_bytes());
        assert_eq!(parse_response(&packet, &origin).unwrap(), unix * 1000);
        assert!(parse_response(&packet, &[8; 8]).is_err());
        packet[0] |= 0xc0;
        assert!(parse_response(&packet, &origin).is_err());
        packet[0] = 0x24;
        packet[1] = 0;
        assert!(parse_response(&packet, &origin).is_err());
        packet[1] = 2;
        let after_wrap = 2_100_000_000u64;
        packet[40..44].copy_from_slice(&((after_wrap + NTP_UNIX_OFFSET) as u32).to_be_bytes());
        assert_eq!(parse_response(&packet, &origin).unwrap(), after_wrap * 1000);
        assert!(parse_response(&packet[..20], &origin).is_err());
    }
}
