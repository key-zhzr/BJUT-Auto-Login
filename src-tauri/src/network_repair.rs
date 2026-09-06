#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdapterRestartTarget {
    pub(crate) interface_name: String,
    pub(crate) ipv4: String,
    #[serde(default)]
    pub(crate) reason: String,
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn validate_target(
    target: &AdapterRestartTarget,
    network: &serde_json::Value,
) -> Result<(), String> {
    let is_en_device = target
        .interface_name
        .strip_prefix("en")
        .is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        });
    let is_lgn_ip = target
        .ipv4
        .parse::<std::net::Ipv4Addr>()
        .is_ok_and(|ip| matches!(ip.octets(), [172, 26, _, _]));
    if !is_en_device
        || !is_lgn_ip
        || network.get("transport").and_then(serde_json::Value::as_str) != Some("ethernet")
        || network
            .get("interfaceName")
            .and_then(serde_json::Value::as_str)
            != Some(target.interface_name.as_str())
        || network.get("ip").and_then(serde_json::Value::as_str) != Some(target.ipv4.as_str())
    {
        return Err("当前有线接口与诊断报告不一致，请重新诊断后再重启".to_string());
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn service_for_interface(list: &str, interface: &str) -> Result<String, String> {
    let mut current = None;
    let mut matches = Vec::new();
    for line in list.lines().map(str::trim) {
        if let Some((_, device)) = line.rsplit_once(", Device: ") {
            if device.trim_end_matches(')') == interface {
                if let Some(service) = current.take() {
                    matches.push(service);
                }
            }
            current = None;
        } else if let Some((index, name)) = line
            .strip_prefix('(')
            .and_then(|line| line.split_once(") "))
        {
            current = (index.parse::<u32>().is_ok() && !name.starts_with('*') && !name.is_empty())
                .then(|| name.to_string());
        }
    }
    if matches.len() != 1 {
        return Err("无法唯一确定此有线适配器的启用服务，请在系统网络设置中检查".to_string());
    }
    Ok(matches.remove(0))
}

#[cfg(any(target_os = "macos", test))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(any(target_os = "macos", test))]
fn restart_script(service: &str, target: &AdapterRestartTarget) -> Result<String, String> {
    if service.is_empty() || service.chars().any(char::is_control) {
        return Err("有线网络服务名称无效".to_string());
    }
    let service = shell_quote(service);
    let interface = shell_quote(&target.interface_name);
    let ip = shell_quote(&target.ipv4);
    // Recheck the address after the system authorization prompt. Always try
    // to restore the service if disabling or re-enabling it fails/interrupted.
    Ok(format!(
        "set -e\n[ \"$(/usr/sbin/ipconfig getifaddr {interface})\" = {ip} ] || exit 70\nrestore() {{ /usr/sbin/networksetup -setnetworkserviceenabled {service} on; }}\ntrap restore EXIT\ntrap 'exit 1' HUP INT TERM\n/usr/sbin/networksetup -setnetworkserviceenabled {service} off\n/bin/sleep 1\nrestore\ntrap - EXIT\n"
    ))
}

#[cfg(target_os = "macos")]
pub(crate) fn restart_ethernet(target: &AdapterRestartTarget) -> Result<String, String> {
    use crate::network_platform::{macos_ipv4_for_interface, macos_is_physical_ethernet_interface};
    if !macos_is_physical_ethernet_interface(&target.interface_name)
        || macos_ipv4_for_interface(&target.interface_name) != target.ipv4
    {
        return Err("有线适配器已发生变化，请重新诊断".to_string());
    }
    let output = std::process::Command::new("/usr/sbin/networksetup")
        .arg("-listnetworkserviceorder")
        .output()
        .map_err(|error| format!("读取网络服务失败：{error}"))?;
    if !output.status.success() {
        return Err("无法读取有线网络服务".to_string());
    }
    let service = service_for_interface(
        &String::from_utf8_lossy(&output.stdout),
        &target.interface_name,
    )?;
    let shell = restart_script(&service, target)?;
    // Arguments carry the shell text without a second AppleScript interpolation
    // layer. macOS owns administrator authentication; no password is collected.
    let output = std::process::Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "on run argv\ndo shell script (item 1 of argv) with administrator privileges\nend run",
            "--",
            &shell,
        ])
        .output()
        .map_err(|error| format!("无法请求系统重启有线适配器：{error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(if error.contains("(-128)") {
            "已取消系统授权，未重启有线适配器".to_string()
        } else if error.contains("(70)") {
            "授权期间有线地址已变化，已停止重启，请重新诊断".to_string()
        } else {
            "有线适配器重启未完成；如服务曾被停用，已尝试重新启用，请检查系统网络设置".to_string()
        });
    }
    Ok(format!(
        "已重启 {service}（{}），正在重新获取网络配置",
        target.interface_name
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> AdapterRestartTarget {
        AdapterRestartTarget {
            interface_name: "en7".to_string(),
            ipv4: "172.26.99.10".to_string(),
            reason: String::new(),
        }
    }

    #[test]
    fn restart_requires_the_same_wired_interface_and_address() {
        let mut network =
            serde_json::json!({"interfaceName":"en7", "ip":"172.26.99.10", "transport":"ethernet"});
        assert!(validate_target(&target(), &network).is_ok());
        network["transport"] = serde_json::json!("wifi");
        assert!(validate_target(&target(), &network).is_err());
        network["transport"] = serde_json::json!("ethernet");
        network["ip"] = serde_json::json!("172.26.99.11");
        assert!(validate_target(&target(), &network).is_err());
        let mut injected = target();
        injected.interface_name = "en7; exit 0".to_string();
        assert!(validate_target(&injected, &network).is_err());
    }

    #[test]
    fn only_one_enabled_service_on_the_exact_device_can_be_restarted() {
        let list = "(1) Wi-Fi\n(Hardware Port: Wi-Fi, Device: en0)\n(2) USB LAN\n(Hardware Port: USB LAN, Device: en7)\n(*) Disabled LAN\n(Hardware Port: USB LAN, Device: en7)\n";
        assert_eq!(service_for_interface(list, "en7").unwrap(), "USB LAN");
        assert!(service_for_interface(list, "en70").is_err());
        assert!(
            service_for_interface("(*) USB LAN\n(Hardware Port: USB LAN, Device: en7)", "en7")
                .is_err()
        );
        let duplicate = format!("{list}(3) Another LAN\n(Hardware Port: USB LAN, Device: en7)");
        assert!(service_for_interface(&duplicate, "en7").is_err());
        assert!(restart_script("invalid\nservice", &target()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_enable_is_retried_by_cleanup_without_touching_another_service() {
        use std::os::unix::fs::PermissionsExt;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder = std::env::temp_dir().join(format!(
            "bjut-network-repair-test-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&folder).unwrap();
        let ipconfig = folder.join("ipconfig");
        let networksetup = folder.join("networksetup");
        let log = folder.join("calls");
        std::fs::write(&ipconfig, "#!/bin/sh\nprintf '%s' 172.26.99.10\n").unwrap();
        std::fs::write(&networksetup, "#!/bin/sh\nprintf '%s:%s\\n' \"$2\" \"$3\" >> \"$BJUT_REPAIR_TEST_LOG\"\nif [ \"$3\" = on ] && [ ! -f \"$BJUT_REPAIR_TEST_LOG.once\" ]; then touch \"$BJUT_REPAIR_TEST_LOG.once\"; exit 55; fi\n").unwrap();
        for path in [&ipconfig, &networksetup] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let service = "USB's $(printf injected) LAN";
        // Only mock executables run; this test cannot disable a real adapter.
        let script = restart_script(service, &target())
            .unwrap()
            .replace(
                "/usr/sbin/ipconfig",
                &shell_quote(ipconfig.to_str().unwrap()),
            )
            .replace(
                "/usr/sbin/networksetup",
                &shell_quote(networksetup.to_str().unwrap()),
            )
            .replace("/bin/sleep 1", ":");
        let result = std::process::Command::new("/bin/sh")
            .args(["-c", &script])
            .env("BJUT_REPAIR_TEST_LOG", &log)
            .output()
            .unwrap();
        assert!(!result.status.success());
        let calls = std::fs::read_to_string(&log).unwrap();
        assert_eq!(
            calls.lines().collect::<Vec<_>>(),
            vec![
                format!("{service}:off"),
                format!("{service}:on"),
                format!("{service}:on")
            ]
        );
        std::fs::remove_file(&log).unwrap();
        let changed_address = script.replace("'172.26.99.10'", "'172.26.99.11'");
        let stopped = std::process::Command::new("/bin/sh")
            .args(["-c", &changed_address])
            .env("BJUT_REPAIR_TEST_LOG", &log)
            .output()
            .unwrap();
        assert_eq!(stopped.status.code(), Some(70));
        assert!(
            !log.exists(),
            "an address change must stop before disabling the service"
        );
        std::fs::remove_dir_all(folder).unwrap();
    }
}
