//! Platform-specific physical-interface discovery helpers.
//!
//! Keeping shell/native interface parsing out of `lib.rs` makes the app
//! orchestration code easier to audit. These helpers never decide whether
//! credentials may be sent; portal response and trust checks remain separate.

#[cfg(any(target_os = "macos", test))]
use super::usable_physical_ipv4;
#[cfg(test)]
use crate::network_trust::is_campus_local_ip;

/// Type 3 gateways live on 172.30/16, but wired clients use BJUT's regular
/// campus address ranges. Callers additionally require physical Ethernet.
#[cfg(test)]
pub(crate) fn is_campus_wired_ipv4(value: &str) -> bool {
    usable_physical_ipv4(value).is_some() && is_campus_local_ip(value)
}

#[cfg(not(target_os = "android"))]
pub(crate) fn is_lgn_wired_client_ipv4(value: &str) -> bool {
    value
        .parse::<std::net::Ipv4Addr>()
        .is_ok_and(|address| matches!(address.octets(), [172, 26, _, _]))
}

#[cfg(target_os = "linux")]
pub(crate) fn split_nmcli_fields(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut escaped = false;
    for character in line.chars() {
        if escaped {
            fields.last_mut().unwrap().push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == ':' {
            fields.push(String::new());
        } else {
            fields.last_mut().unwrap().push(character);
        }
    }
    if escaped {
        fields.last_mut().unwrap().push('\\');
    }
    fields
}

#[cfg(target_os = "macos")]
pub(crate) fn macos_route_interface(destination: &str) -> String {
    let output = if destination.contains(':') {
        macos_read_command("/sbin/route", &["-n", "get", "-inet6", destination])
    } else {
        macos_read_command("/sbin/route", &["-n", "get", destination])
    };
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("interface:").map(str::trim))
        .unwrap_or_default()
        .to_owned()
}

#[cfg(target_os = "macos")]
pub(crate) fn macos_ipv4_for_interface(interface: &str) -> String {
    if interface.is_empty() {
        return String::new();
    }
    let output = macos_read_command("/usr/sbin/ipconfig", &["getifaddr", interface]);
    let candidate = output.trim();
    if usable_physical_ipv4(candidate).is_some() {
        candidate.to_owned()
    } else {
        String::new()
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn macos_is_physical_ethernet_interface(interface: &str) -> bool {
    interface.starts_with("en")
        && corewlan::WiFiClient::shared()
            .map(|client| {
                !client
                    .interface_names()
                    .iter()
                    .any(|name| name == interface)
            })
            .unwrap_or(false)
}

#[cfg(target_os = "macos")]
pub(crate) fn macos_read_command(program: &str, args: &[&str]) -> String {
    macos_command_output(program, args, std::time::Duration::from_secs(2)).unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn macos_command_output(
    program: &str,
    args: &[&str],
    timeout: std::time::Duration,
) -> Option<String> {
    use std::{
        io::{ErrorKind, Read},
        os::{fd::OwnedFd, unix::net::UnixStream},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    // route can wait in the kernel indefinitely. Read concurrently with the
    // command and count every read and the exit wait against one deadline.
    let deadline = Instant::now() + timeout;
    let (mut reader, writer) = UnixStream::pair().ok()?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(OwnedFd::from(writer)))
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let result = (|| {
        let remaining = || deadline.checked_duration_since(Instant::now());
        let mut output = Vec::new();
        let mut buffer = [0; 8192];
        loop {
            reader.set_read_timeout(Some(remaining()?)).ok()?;
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) if output.len() + count <= 1024 * 1024 => {
                    output.extend_from_slice(&buffer[..count]);
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                _ => return None,
            }
        }
        loop {
            if let Some(status) = child.try_wait().ok()? {
                return status
                    .success()
                    .then(|| String::from_utf8_lossy(&output).into_owned());
            }
            std::thread::sleep(remaining()?.min(Duration::from_millis(10)));
        }
    })();
    if result.is_none() && child.try_wait().ok().flatten().is_none() && child.kill().is_ok() {
        let _ = child.wait();
    }
    result
}

#[cfg(all(test, target_os = "macos"))]
mod macos_command_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn terminates_a_stalled_command_even_after_it_closes_stdout() {
        for script in ["exec /bin/sleep 20", "exec 1>&-; exec /bin/sleep 20"] {
            let started = Instant::now();
            assert_eq!(
                macos_command_output("/bin/sh", &["-c", script], Duration::from_millis(100)),
                None
            );
            assert!(started.elapsed() < Duration::from_secs(2));
        }
    }

    #[test]
    fn drains_large_output_and_accepts_only_successful_commands() {
        let output = macos_command_output(
            "/usr/bin/awk",
            &["BEGIN{for(i=0;i<100000;i++)printf \"xy\"}"],
            Duration::from_secs(10),
        )
        .expect("successful command should return its complete output");
        assert_eq!(output.len(), 200_000);
        assert!(macos_read_command("/bin/sh", &["-c", "printf invalid; exit 1"]).is_empty());
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_is_physical_interface(interface: &str) -> bool {
    let sysfs = std::path::Path::new("/sys/class/net").join(interface);
    sysfs.join("device").exists()
}
