use std::{
    io::Read,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
static BUSY: AtomicBool = AtomicBool::new(false);
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TraceResult {
    target: String,
    available: bool,
    timed_out: bool,
    hops: Vec<Hop>,
    output: String,
}
#[derive(serde::Serialize)]
struct Hop {
    index: u32,
    address: Option<String>,
    detail: String,
}
fn target(key: &str) -> Option<&'static str> {
    match key {
        "campus" => Some("172.30.201.2"),
        "billing" => Some("172.21.0.16"),
        "internet4" => Some("223.5.5.5"),
        "internet6" => Some("2400:3200::1"),
        _ => None,
    }
}
fn hops(text: &str) -> Vec<Hop> {
    text.lines()
        .filter_map(|line| {
            let mut cells = line.split_whitespace();
            let index = cells
                .next()?
                .parse::<u32>()
                .ok()
                .filter(|v| (1..=16).contains(v))?;
            let address = cells.find_map(|cell| {
                cell.trim_matches(['(', ')', '[', ']'])
                    .parse::<std::net::IpAddr>()
                    .ok()
                    .map(|v| v.to_string())
            });
            Some(Hop {
                index,
                address,
                detail: line.chars().take(256).collect(),
            })
        })
        .take(16)
        .collect()
}
fn trace(key: String) -> Result<TraceResult, String> {
    let address = target(&key).ok_or("所选链路目标无效")?;
    let ipv6 = address.contains(':');
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = Command::new("tracert");
        c.args([
            "-d",
            "-h",
            "8",
            "-w",
            "800",
            if ipv6 { "-6" } else { "-4" },
            address,
        ]);
        c
    };
    #[cfg(not(target_os = "windows"))]
    let mut command = {
        let mut c = Command::new(if ipv6 && cfg!(target_os = "macos") {
            "traceroute6"
        } else {
            "traceroute"
        });
        if ipv6 && !cfg!(target_os = "macos") {
            c.arg("-6");
        }
        c.args(["-n", "-m", "8", "-w", "1", "-q", "1", address]);
        c
    };
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = match command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TraceResult {
                target: address.into(),
                available: false,
                timed_out: false,
                hops: vec![],
                output: "此系统未提供链路追踪工具，可继续使用常规网络诊断。".into(),
            })
        }
        Err(_) => return Err("无法启动链路检查".into()),
    };
    let start = Instant::now();
    let mut timed_out = false;
    loop {
        if child
            .try_wait()
            .map_err(|_| "无法读取链路检查状态")?
            .is_some()
        {
            break;
        }
        if start.elapsed() > Duration::from_secs(14) {
            timed_out = true;
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut bytes = Vec::new();
    if let Some(out) = child.stdout.take() {
        let _ = out.take(16384).read_to_end(&mut bytes);
    }
    if bytes.is_empty() {
        if let Some(out) = child.stderr.take() {
            let _ = out.take(2048).read_to_end(&mut bytes);
        }
    }
    let output = String::from_utf8_lossy(&bytes).into_owned();
    Ok(TraceResult {
        target: address.into(),
        available: true,
        timed_out,
        hops: hops(&output),
        output,
    })
}
#[tauri::command]
pub(crate) async fn trace_network(target_key: String) -> Result<TraceResult, String> {
    target(&target_key).ok_or("所选链路目标无效")?;
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("已有链路检查正在进行，请稍后重试".into());
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            BUSY.store(false, Ordering::SeqCst);
        }
    }
    let guard = Guard;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        trace(target_key)
    })
    .await
    .map_err(|_| "链路检查未完成".to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_fixed_targets_and_bounded_hops() {
        assert!(target("example.com;echo bad").is_none());
        let rows = hops(" 1 192.0.2.1 1 ms\n 2 * * *\n 3 2001:db8::1 2 ms\n 99 malformed");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].address.as_deref(), Some("192.0.2.1"));
        assert!(rows[1].address.is_none());
    }
}
