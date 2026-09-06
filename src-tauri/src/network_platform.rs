//! Platform-specific physical-interface discovery helpers.
//!
//! Keeping shell/native interface parsing out of `lib.rs` makes the app
//! orchestration code easier to audit. These helpers never decide whether
//! credentials may be sent; portal response and trust checks remain separate.

use super::usable_physical_ipv4;
use crate::network_trust::is_campus_local_ip;

/// Type 3 gateways live on 172.30/16, but wired clients use BJUT's regular
/// campus address ranges. Callers additionally require physical Ethernet.
#[cfg(not(target_os = "android"))]
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
    std::process::Command::new("route")
        .args(["-n", "get", destination])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .find_map(|line| line.trim().strip_prefix("interface:").map(str::trim))
                .map(str::to_string)
        })
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
pub(crate) fn macos_ipv4_for_interface(interface: &str) -> String {
    if interface.is_empty() {
        return String::new();
    }
    std::process::Command::new("ipconfig")
        .args(["getifaddr", interface])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|candidate| usable_physical_ipv4(candidate).is_some())
        .unwrap_or_default()
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
pub(crate) fn macos_campus_wired_identity(excluded_interface: &str) -> Option<(String, String)> {
    let output = std::process::Command::new("ifconfig")
        .arg("-l")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter(|interface| {
            macos_is_physical_ethernet_interface(interface) && *interface != excluded_interface
        })
        .find_map(|interface| {
            let address = macos_ipv4_for_interface(interface);
            is_campus_wired_ipv4(&address).then(|| (interface.to_string(), address))
        })
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_is_physical_interface(interface: &str) -> bool {
    let sysfs = std::path::Path::new("/sys/class/net").join(interface);
    sysfs.join("device").exists()
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_is_physical_ethernet_interface(interface: &str) -> bool {
    linux_is_physical_interface(interface)
        && !std::path::Path::new("/sys/class/net")
            .join(interface)
            .join("wireless")
            .exists()
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_route_identity(destination: &str) -> Option<(String, String)> {
    let output = std::process::Command::new("ip")
        .args(["-4", "route", "get", destination])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let output = String::from_utf8_lossy(&output.stdout);
    let fields = output.split_whitespace().collect::<Vec<_>>();
    let interface = fields
        .windows(2)
        .find(|pair| pair[0] == "dev")
        .map(|pair| pair[1])?;
    let address = fields
        .windows(2)
        .find(|pair| pair[0] == "src")
        .map(|pair| pair[1])?;
    (linux_is_physical_interface(interface) && usable_physical_ipv4(address).is_some())
        .then(|| (interface.to_string(), address.to_string()))
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_campus_wired_identity(excluded_interface: &str) -> Option<(String, String)> {
    let output = std::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show", "scope", "global"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            let interface = fields.get(1)?.trim_end_matches(':');
            if !linux_is_physical_ethernet_interface(interface) || interface == excluded_interface {
                return None;
            }
            let address = fields
                .windows(2)
                .find(|pair| pair[0] == "inet")?
                .get(1)?
                .split('/')
                .next()?;
            is_campus_wired_ipv4(address).then(|| (interface.to_string(), address.to_string()))
        })
}
