//! Read-only interface inventory. A saved selection is resolved exactly and
//! never falls back to a different adapter when the chosen link disappears.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NetworkAdapter {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) interface_name: String,
    pub(crate) transport: String,
    pub(crate) ipv4: Vec<String>,
    pub(crate) ipv6: Vec<String>,
    // Used internally by direct probes, not displayed/exported with inventory.
    #[serde(default, skip_serializing)]
    pub(crate) dns_servers: Vec<String>,
    pub(crate) connected: bool,
    pub(crate) selectable: bool,
    #[serde(default)]
    pub(crate) selected: bool,
}

pub(crate) fn usable_ipv6(value: &str) -> bool {
    value.parse::<std::net::Ipv6Addr>().is_ok_and(|ip| {
        !ip.is_unspecified()
            && !ip.is_loopback()
            && !ip.is_multicast()
            && !ip.is_unicast_link_local()
            && ip.to_ipv4_mapped().is_none()
    })
}

pub(crate) fn preferred_adapter<'a>(
    adapters: &'a [NetworkAdapter],
    preferred: &str,
) -> Option<&'a NetworkAdapter> {
    adapters.iter().find(|adapter| {
        adapter.id == preferred
            && adapter.selectable
            && adapter.connected
            && adapter
                .ipv4
                .iter()
                .any(|ip| crate::usable_physical_ipv4(ip).is_some())
    })
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn read(program: &str, args: &[&str]) -> String {
    std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

#[cfg(any(target_os = "macos", test))]
fn parse_ifconfig(text: &str, wifi_names: &[String]) -> Vec<NetworkAdapter> {
    let mut adapters: Vec<NetworkAdapter> = Vec::new();
    let mut skip = false;
    for line in text.lines() {
        if !line.starts_with(char::is_whitespace) {
            skip = true;
            let Some((name, flags)) = line.split_once(':') else {
                continue;
            };
            if name == "lo0" || name.is_empty() {
                continue;
            }
            skip = false;
            let wifi = wifi_names.iter().any(|item| item == name);
            let physical = name.strip_prefix("en").is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
            });
            adapters.push(NetworkAdapter {
                id: name.to_string(),
                name: name.to_string(),
                interface_name: name.to_string(),
                transport: if wifi {
                    "wifi"
                } else if physical {
                    "ethernet"
                } else {
                    "vpn"
                }
                .to_string(),
                selectable: physical,
                connected: flags.contains("UP,"),
                ..Default::default()
            });
        } else if let Some(adapter) = adapters.last_mut().filter(|_| !skip) {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.first() == Some(&"status:") && fields.get(1) == Some(&"inactive") {
                adapter.connected = false;
            }
            if fields.first() == Some(&"inet") {
                if let Some(ip) = fields.get(1).filter(|ip| {
                    ip.parse::<std::net::Ipv4Addr>()
                        .is_ok_and(|ip| !ip.is_unspecified() && !ip.is_loopback())
                }) {
                    adapter.ipv4.push(ip.to_string());
                }
            } else if fields.first() == Some(&"inet6")
                && !fields.iter().any(|flag| {
                    ["tentative", "duplicated", "detached", "deprecated"].contains(flag)
                })
            {
                if let Some(ip) = fields.get(1).filter(|ip| usable_ipv6(ip)) {
                    adapter.ipv6.push(ip.to_string());
                }
            }
        }
    }
    adapters.retain(|adapter| {
        adapter.selectable || !adapter.ipv4.is_empty() || !adapter.ipv6.is_empty()
    });
    adapters
}

#[cfg(target_os = "macos")]
pub(crate) fn adapters() -> Vec<NetworkAdapter> {
    let wifi = corewlan::WiFiClient::shared()
        .map(|client| client.interface_names())
        .unwrap_or_default();
    let mut adapters = parse_ifconfig(&read("/sbin/ifconfig", &[]), &wifi);
    let ports = read("/usr/sbin/networksetup", &["-listallhardwareports"]);
    let mut name = "";
    for line in ports.lines() {
        if let Some(value) = line.strip_prefix("Hardware Port: ") {
            name = value;
        }
        if let Some(device) = line.strip_prefix("Device: ") {
            if let Some(adapter) = adapters.iter_mut().find(|adapter| adapter.id == device) {
                adapter.name = name.to_string();
            }
        }
    }
    adapters
}

#[cfg(target_os = "linux")]
pub(crate) fn adapters() -> Vec<NetworkAdapter> {
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(&read("ip", &["-j", "address", "show"])).unwrap_or_default();
    rows.into_iter()
        .filter_map(|row| {
            let name = row["ifname"].as_str()?;
            if name == "lo" {
                return None;
            }
            let physical = crate::network_platform::linux_is_physical_interface(name);
            let wifi = std::path::Path::new("/sys/class/net")
                .join(name)
                .join("wireless")
                .exists();
            let mut adapter = NetworkAdapter {
                id: name.to_string(),
                name: name.to_string(),
                interface_name: name.to_string(),
                transport: if wifi {
                    "wifi"
                } else if physical {
                    "ethernet"
                } else {
                    "vpn"
                }
                .to_string(),
                connected: row["flags"]
                    .as_array()
                    .is_some_and(|flags| flags.iter().any(|flag| flag == "UP")),
                selectable: physical,
                ..Default::default()
            };
            for address in row["addr_info"].as_array().into_iter().flatten() {
                if address["tentative"] == true
                    || address["deprecated"] == true
                    || address["dadfailed"] == true
                {
                    continue;
                }
                let ip = address["local"].as_str().unwrap_or("");
                if address["family"] == "inet" && crate::usable_physical_ipv4(ip).is_some() {
                    adapter.ipv4.push(ip.to_string());
                }
                if address["family"] == "inet6" && usable_ipv6(ip) {
                    adapter.ipv6.push(ip.to_string());
                }
            }
            Some(adapter)
        })
        .collect()
}

#[cfg(target_os = "windows")]
pub(crate) fn adapters() -> Vec<NetworkAdapter> {
    windows_inventory::adapters()
}

#[cfg(target_os = "windows")]
mod windows_inventory;

#[cfg(target_os = "android")]
pub(crate) fn adapters() -> Vec<NetworkAdapter> {
    serde_json::from_str(&crate::call_android_network_helper_string(
        "getNetworkAdapters",
    ))
    .unwrap_or_default()
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "windows",
    target_os = "android"
)))]
pub(crate) fn adapters() -> Vec<NetworkAdapter> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn android_inventory_without_dns_still_deserializes_and_resolvers_stay_internal() {
        let mut adapter: NetworkAdapter = serde_json::from_value(serde_json::json!({
            "id":"101", "name":"wlan0", "interfaceName":"wlan0", "transport":"wifi",
            "ipv4":["192.168.1.216"], "ipv6":[], "connected":true, "selectable":false
        }))
        .unwrap();
        assert!(adapter.dns_servers.is_empty());
        adapter.dns_servers.push("192.168.1.1".into());
        assert!(serde_json::to_value(adapter)
            .unwrap()
            .get("dnsServers")
            .is_none());
    }

    #[test]
    fn inventory_keeps_interfaces_and_address_families_separate() {
        let adapters = parse_ifconfig("en0: flags=UP,RUNNING\n inet 192.168.1.10\n status: active\nen7: flags=UP,RUNNING\n inet 172.26.99.10\n inet6 2001:db8::10\n inet6 2001:db8::11 tentative\n status: active\nutun4: flags=UP,RUNNING\n inet 198.18.0.1\n inet6 fd00::1\n", &["en0".to_string()]);
        assert_eq!(adapters.len(), 3);
        let wired = preferred_adapter(&adapters, "en7").unwrap();
        assert_eq!(wired.ipv6, ["2001:db8::10"]);
        assert_eq!(wired.transport, "ethernet");
        assert!(preferred_adapter(&adapters, "utun4").is_none());
        assert!(preferred_adapter(&adapters, "en9").is_none());
        let mut disconnected = adapters;
        disconnected[1].connected = false;
        assert!(preferred_adapter(&disconnected, "en7").is_none());
        assert!(preferred_adapter(&disconnected, "en0").is_some());
    }
}
