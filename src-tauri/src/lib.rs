// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn get_network_info(_app: tauri::AppHandle) -> serde_json::Value {
    #[cfg(target_os = "android")]
    {
        let mut result = serde_json::json!({"ssid": "", "bssid": "", "ip": ""});
        if let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() {
            if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) } {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let activity = unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                    
                    match tauri::wry::prelude::find_class(&mut env, &activity, "cn.edu.bjut.al.NetworkHelper".into()) {
                        Ok(class) => {
                            let method_call = env.call_static_method(
                                class,
                                "getNetworkInfo",
                                "(Landroid/content/Context;)Ljava/lang/String;",
                                &[jni::objects::JValue::Object(&activity)],
                            );
                            
                            match method_call {
                                Ok(jvalue) => {
                                    if let Ok(jobject) = jvalue.l() {
                                        let jstring: jni::objects::JString = jobject.into();
                                        if let Ok(rust_str) = env.get_string(&jstring).map(|s| { let s: String = s.into(); s }) {
                                            if let Ok(val) = serde_json::from_str(&rust_str) {
                                                result = val;
                                            }
                                        }
                                    }
                                }
                                Err(_) => {
                                    if env.exception_check().unwrap_or(false) {
                                        let _ = env.exception_clear();
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            if env.exception_check().unwrap_or(false) {
                                let _ = env.exception_clear();
                            }
                        }
                    }
                    
                    if env.exception_check().unwrap_or(false) {
                        let _ = env.exception_clear();
                    }
                }
            }
        }
        return result;
    }

    #[cfg(not(target_os = "android"))]
    {
        let mut ssid = String::new();
        let mut bssid = String::new();
        let mut ip = String::new();

        #[cfg(target_os = "macos")]
        {
            if let Ok(client) = corewlan::WiFiClient::shared() {
                if let Some(interface) = client.interface() {
                    if let Some(ssid_str) = interface.ssid() {
                        ssid = ssid_str;
                    }
                    if let Some(bssid_str) = interface.bssid() {
                        bssid = bssid_str;
                    }
                }
            }

            if let Ok(output) = std::process::Command::new("sh").arg("-c").arg("ipconfig getifaddr en0").output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let trimmed = stdout.trim();
                if !trimmed.is_empty() {
                    ip = trimmed.to_string();
                } else if let Ok(output2) = std::process::Command::new("sh").arg("-c").arg("ipconfig getifaddr en1").output() {
                    ip = String::from_utf8_lossy(&output2.stdout).trim().to_string();
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            // Get SSID/BSSID via netsh wlan (doesn't trigger location prompts)
            let mut cmd = std::process::Command::new("netsh");
            cmd.args(["wlan", "show", "interfaces"]);
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            if let Ok(output) = cmd.output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let trimmed = line.trim();
                    let upper = trimmed.to_uppercase();
                    if upper.contains("BSSID") {
                        if let Some(idx) = trimmed.find(':') {
                            bssid = trimmed[idx + 1..].trim().to_string();
                        }
                    } else if upper.contains("SSID") {
                        if let Some(idx) = trimmed.find(':') {
                            ssid = trimmed[idx + 1..].trim().to_string();
                        }
                    }
                }
            }

            // Get IP via rust ipconfig (avoids location prompts and VM startup overhead)
            let mut ipconfig_ips = Vec::new();
            let mut ip_cmd = std::process::Command::new("ipconfig");
            ip_cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            if let Ok(output) = ip_cmd.output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if line.contains("IPv4") {
                        if let Some(idx) = line.find(':') {
                            let extracted_ip = line[idx + 1..].trim().to_string();
                            if !extracted_ip.is_empty() {
                                ipconfig_ips.push(extracted_ip);
                            }
                        }
                    }
                }
            }
            let mut best_ip = String::new();
            for extracted_ip in &ipconfig_ips {
                if extracted_ip.starts_with("10.") || extracted_ip.starts_with("172.") {
                    best_ip = extracted_ip.clone();
                    break;
                }
            }
            if best_ip.is_empty() && !ipconfig_ips.is_empty() {
                for extracted_ip in &ipconfig_ips {
                    if !extracted_ip.starts_with("198.18.") && !extracted_ip.starts_with("127.") {
                        best_ip = extracted_ip.clone();
                        break;
                    }
                }
            }
            if !best_ip.is_empty() {
                ip = best_ip;
            }
        }

        #[cfg(target_os = "linux")]
        {
            if let Ok(output) = std::process::Command::new("nmcli")
                .args(["-t", "-f", "active,ssid,bssid", "dev", "wifi"])
                .output()
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if line.starts_with("yes:") {
                        let parts: Vec<&str> = line.split(':').collect();
                        if parts.len() >= 3 {
                            ssid = parts[1].to_string();
                            let raw_bssid = parts[2..].join(":");
                            bssid = raw_bssid.replace("\\:", ":");
                        }
                    }
                }
            }

            if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
                if socket.connect("8.8.8.8:80").is_ok() {
                    if let Ok(local_addr) = socket.local_addr() {
                        ip = local_addr.ip().to_string();
                    }
                }
            }
        }

        serde_json::json!({
            "ssid": ssid,
            "bssid": bssid,
            "ip": ip
        })
    }
}

#[tauri::command]
fn request_battery_optimizations(_app: tauri::AppHandle) {
    #[cfg(target_os = "android")]
    {
        if let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() {
            if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) } {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let activity = unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                    
                    let call = env.call_method(
                        &activity,
                        "requestBatteryOptimizations",
                        "()V",
                        &[],
                    );
                    if call.is_err() {
                        if env.exception_check().unwrap_or(false) {
                            let _ = env.exception_clear();
                        }
                    }
                }
            }
        }
    }
}

#[tauri::command]
fn request_foreground_permissions(_app: tauri::AppHandle) {
    #[cfg(target_os = "android")]
    {
        if let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() {
            if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) } {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let activity = unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                    let _ = env.call_method(
                        &activity,
                        "requestForegroundPermissions",
                        "()V",
                        &[],
                    );
                    if env.exception_check().unwrap_or(false) {
                        let _ = env.exception_clear();
                    }
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        static PROMPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !PROMPTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let _ = _app.run_on_main_thread(|| {
                unsafe {
                    let manager = objc2_core_location::CLLocationManager::new();
                    manager.requestWhenInUseAuthorization();
                    manager.startUpdatingLocation();
                    let _ = Box::leak(Box::new(manager));
                }
            });
        }
    }
}

#[tauri::command]
fn request_background_permissions(_app: tauri::AppHandle) {
    #[cfg(target_os = "android")]
    {
        if let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() {
            if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) } {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let activity = unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                    let _ = env.call_method(
                        &activity,
                        "requestBackgroundPermissions",
                        "()V",
                        &[],
                    );
                    if env.exception_check().unwrap_or(false) {
                        let _ = env.exception_clear();
                    }
                }
            }
        }
    }
}

#[tauri::command]
fn start_keep_alive_service(_app: tauri::AppHandle) {
    #[cfg(target_os = "android")]
    {
        if let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() {
            if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) } {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let activity = unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                    let _ = env.call_method(
                        &activity,
                        "startKeepAliveService",
                        "()V",
                        &[],
                    );
                    if env.exception_check().unwrap_or(false) {
                        let _ = env.exception_clear();
                    }
                }
            }
        }
    }
}

#[tauri::command]
fn stop_keep_alive_service(_app: tauri::AppHandle) {
    #[cfg(target_os = "android")]
    {
        if let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() {
            if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) } {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let activity = unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                    let _ = env.call_method(
                        &activity,
                        "stopKeepAliveService",
                        "()V",
                        &[],
                    );
                    if env.exception_check().unwrap_or(false) {
                        let _ = env.exception_clear();
                    }
                }
            }
        }
    }
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn set_dock_visible(app: tauri::AppHandle, visible: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use tauri::ActivationPolicy;
        let policy = if visible {
            ActivationPolicy::Regular
        } else {
            ActivationPolicy::Accessory
        };
        app.set_activation_policy(policy).map_err(|e| e.to_string())?;
        if visible {
            // Changing Accessory -> Regular is asynchronous in AppKit. Restore
            // focus after the policy transition instead of during cold start.
            let delayed_app = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(180)).await;
                let main_thread_app = delayed_app.clone();
                let _ = delayed_app.run_on_main_thread(move || {
                    if let Some(window) = main_thread_app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                });
            });
        }
    }
    let _ = app;
    let _ = visible;
    Ok(())
}

#[tauri::command]
fn frontend_ready(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if let Some(window) = app.get_webview_window("main") {
            window.show().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
        }
    }
    let _ = app;
    Ok(())
}

#[tauri::command]
fn get_local_ip() -> String {
    let mut ip = String::new();
    
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut ipconfig_ips = Vec::new();
        let mut ip_cmd = std::process::Command::new("ipconfig");
        ip_cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        if let Ok(output) = ip_cmd.output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("IPv4") {
                    if let Some(idx) = line.find(':') {
                        let extracted_ip = line[idx + 1..].trim().to_string();
                        if !extracted_ip.is_empty() {
                            ipconfig_ips.push(extracted_ip);
                        }
                    }
                }
            }
        }
        let mut best_ip = String::new();
        for extracted_ip in &ipconfig_ips {
            if extracted_ip.starts_with("10.") || extracted_ip.starts_with("172.") {
                best_ip = extracted_ip.clone();
                break;
            }
        }
        if best_ip.is_empty() && !ipconfig_ips.is_empty() {
            for extracted_ip in &ipconfig_ips {
                if !extracted_ip.starts_with("198.18.") && !extracted_ip.starts_with("127.") {
                    best_ip = extracted_ip.clone();
                    break;
                }
            }
        }
        if !best_ip.is_empty() {
            ip = best_ip;
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(target_os = "android")]
        {
            if let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() {
                if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) } {
                    if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                        let activity = unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                        if let Ok(class) = tauri::wry::prelude::find_class(&mut env, &activity, "cn.edu.bjut.al.NetworkHelper".into()) {
                            let method_call = env.call_static_method(
                                class,
                                "getLocalIpAddress",
                                "()Ljava/lang/String;",
                                &[],
                            );
                            if let Ok(jvalue) = method_call {
                                if let Ok(jobject) = jvalue.l() {
                                    let jstring: jni::objects::JString = jobject.into();
                                    if let Ok(rust_str) = env.get_string(&jstring).map(|s| { let s: String = s.into(); s }) {
                                        ip = rust_str;
                                    }
                                }
                            }
                        }
                        if env.exception_check().unwrap_or(false) { let _ = env.exception_clear(); }
                    }
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Ok(output) = std::process::Command::new("sh").arg("-c").arg("ipconfig getifaddr en0").output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let trimmed = stdout.trim();
                if !trimmed.is_empty() {
                    ip = trimmed.to_string();
                } else if let Ok(output2) = std::process::Command::new("sh").arg("-c").arg("ipconfig getifaddr en1").output() {
                    ip = String::from_utf8_lossy(&output2.stdout).trim().to_string();
                }
            }
        }

        #[cfg(not(any(target_os = "windows", target_os = "android", target_os = "macos")))]
        {
            if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
                if socket.connect("8.8.8.8:80").is_ok() {
                    if let Ok(local_addr) = socket.local_addr() {
                        ip = local_addr.ip().to_string();
                    }
                }
            }
        }
    }
    
    ip
}

#[tauri::command]
fn read_clipboard() -> Result<String, String> {
    #[cfg(not(target_os = "android"))]
    {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        clipboard.get_text().map_err(|e| e.to_string())
    }
    #[cfg(target_os = "android")]
    {
        Ok(String::new())
    }
}

#[tauri::command]
fn write_clipboard(text: String) -> Result<(), String> {
    #[cfg(not(target_os = "android"))]
    {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        clipboard.set_text(text).map_err(|e| e.to_string())
    }
    #[cfg(target_os = "android")]
    {
        let _ = text;
        Ok(())
    }
}

use std::collections::HashMap;
use std::sync::{Mutex, RwLock, Arc};
use std::sync::atomic::{AtomicI32, AtomicBool, AtomicU32, Ordering};
use tauri::Emitter;
use tauri::Manager;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
struct Account {
    #[serde(alias = "username")]
    user: String,
    #[serde(alias = "password")]
    pass: String,
    #[serde(default, rename = "isDefault", alias = "is_default")]
    is_default: bool,
    #[serde(default, rename = "isDisabled", alias = "is_disabled")]
    is_disabled: Option<bool>,
}

#[derive(serde::Serialize)]
struct ManualLoginResult {
    success: bool,
    message: String,
}

#[derive(serde::Serialize)]
struct UserInfo {
    account: String,
    balance: String,
    flow: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateTarget {
    platform: String,
    arch: String,
    format: String,
    current_version: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct AccountHealth {
    #[serde(default)]
    consecutive_failures: u32,
    #[serde(default)]
    cooldown_until: Option<i64>,
    #[serde(default)]
    last_success: Option<String>,
    #[serde(default)]
    last_failure: Option<String>,
    #[serde(default)]
    last_failure_reason: Option<String>,
    #[serde(default)]
    failure_kind: Option<String>,
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct AccountHealthView {
    user: String,
    status: String,
    consecutive_failures: u32,
    cooldown_until: Option<i64>,
    cooldown_seconds: i64,
    last_success: Option<String>,
    last_failure: Option<String>,
    last_failure_reason: Option<String>,
    failure_kind: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CredentialStorageHealth {
    status: String,
    backend: String,
    persistent: bool,
    saved_accounts: usize,
    missing_password_accounts: Vec<String>,
    message: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticStep {
    id: String,
    label: String,
    status: String,
    message: String,
    duration_ms: u128,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticReport {
    created_at: String,
    overall: String,
    summary: String,
    ssid: String,
    ip: String,
    steps: Vec<DiagnosticStep>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
struct AppConfig {
    #[serde(default)]
    accounts: Vec<Account>,
    #[serde(default = "default_auto_login", alias = "autoLogin")]
    auto_login: bool,
    #[serde(default = "default_check_interval", alias = "checkInterval")]
    check_interval: i32,
    #[serde(default = "default_check_interval_bg", alias = "checkIntervalBg")]
    check_interval_bg: i32,
    #[serde(default = "default_wifi_change_detect", alias = "wifiChangeDetect")]
    wifi_change_detect: bool,
    #[serde(default = "default_log_level", alias = "logLevel")]
    log_level: String,
    #[serde(default)]
    whitelist: Vec<String>,
    #[serde(default)]
    blacklist: Vec<String>,
}

fn default_auto_login() -> bool { false }
fn default_check_interval() -> i32 { 15 }
fn default_check_interval_bg() -> i32 { 60 }
fn default_wifi_change_detect() -> bool { true }
fn default_log_level() -> String { "info".to_string() }

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct LogEntry {
    time: String,
    module: String,
    message: String,
    #[serde(rename = "type")]
    log_type: String, // "info" | "error" | "success" | "debug"
}

struct AppState {
    config: RwLock<AppConfig>,
    credential_storage_status: Mutex<String>,
    account_health: Mutex<HashMap<String, AccountHealth>>,
    logs: Mutex<Vec<LogEntry>>,
    countdown: AtomicI32,
    is_checking: AtomicBool,
    pending_full_check: AtomicBool,
    is_suspended: AtomicBool,
    last_known_ip: Mutex<Option<String>>,
    non_campus_count: AtomicU32,
    is_in_background: AtomicBool,
    last_network_state: Mutex<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq)]
enum LoginType {
    Type1, // 10.21.221.98 (eportal)
    Type2, // 10.21.251.3 (drcom)
    Type3, // lgn.bjut.edu.cn
    Unknown,
}

impl LoginType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Type1 => "Type1_221_98",
            Self::Type2 => "Type2_251_3",
            Self::Type3 => "Type3_172_30",
            Self::Unknown => "Unknown",
        }
    }
}

fn is_campus_local_ip(ip: &str) -> bool {
    let octets: Vec<u8> = ip.split('.')
        .map(str::parse::<u8>)
        .collect::<Result<_, _>>()
        .unwrap_or_default();
    if octets.len() != 4 {
        return false;
    }
    match (octets[0], octets[1]) {
        (10, 17..=27) | (10, 121) | (10, 126) | (10, 226) | (172, 17..=27) => true,
        _ => false,
    }
}

fn is_known_campus_ssid(ssid: &str) -> bool {
    let normalized = ssid.trim().to_ascii_lowercase().replace('_', "-");
    normalized == "bjut-wifi" || normalized == "bjut-sushe"
}

fn automatic_login_network_allowed(
    login_type: &LoginType,
    ssid: &str,
    bssid: &str,
    ip: &str,
    whitelist: &[String],
    blacklist: &[String],
) -> Result<(), String> {
    let normalized_ssid = ssid.trim();
    let net_key = format!("{}|{}", normalized_ssid, bssid);
    if blacklist.contains(&net_key) {
        return Err(format!("当前网络 ({normalized_ssid}) 在黑名单中"));
    }
    if whitelist.contains(&net_key) {
        return Ok(());
    }
    if !is_campus_local_ip(ip) {
        return Err("本地 IP 不属于已知校园网网段".to_string());
    }

    match login_type {
        LoginType::Type1 | LoginType::Type2 if is_known_campus_ssid(normalized_ssid) => Ok(()),
        LoginType::Type3
            if normalized_ssid.is_empty()
                || normalized_ssid.eq_ignore_ascii_case("unknown")
                || normalized_ssid.eq_ignore_ascii_case("<unknown ssid>")
                || is_known_campus_ssid(normalized_ssid) => Ok(()),
        LoginType::Type1 | LoginType::Type2 => {
            Err("无线网络名称未经识别，且未加入白名单".to_string())
        }
        LoginType::Type3 => Err("当前无线网络未经识别，且未加入白名单".to_string()),
        LoginType::Unknown => Err("未识别到校园网认证协议".to_string()),
    }
}

fn url_encode(input: &str) -> String {
    let mut output = String::new();
    for b in input.as_bytes() {
        match *b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(*b as char);
            }
            _ => {
                output.push_str(&format!("%{:02X}", b));
            }
        }
    }
    output
}

fn find_v6ip(html: &str) -> String {
    if let Some(name_pos) = html.find("name=\"v6ip\"") {
        let substring = &html[name_pos..];
        if let Some(val_pos) = substring.find("value=\"") {
            let val_start = val_pos + 7;
            if let Some(val_end) = substring[val_start..].find('"') {
                return substring[val_start..val_start + val_end].to_string();
            }
        }
    }
    if let Some(name_pos) = html.find("name='v6ip'") {
        let substring = &html[name_pos..];
        if let Some(val_pos) = substring.find("value='") {
            let val_start = val_pos + 7;
            if let Some(val_end) = substring[val_start..].find('\'') {
                return substring[val_start..val_start + val_end].to_string();
            }
        }
    }
    String::new()
}

fn parse_dr_response(text: &str) -> (bool, String) {
    if let Some(start_idx) = text.find('(') {
        if let Some(end_idx) = text.rfind(')') {
            let json_str = &text[start_idx + 1..end_idx];
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Some(result) = data.get("result").and_then(|v| v.as_i64()) {
                    if result == 1 {
                        return (true, "Portal协议认证成功！".to_string());
                    } else {
                        let msg = data.get("msg")
                            .or_else(|| data.get("msga"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("未知错误")
                            .to_string();
                        return (false, msg);
                    }
                }
            }
        }
    }
    (false, "解析响应数据失败".to_string())
}

async fn check_internet_rust() -> bool {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(1500))
        .build();
    if let Ok(c) = client {
        if let Ok(res) = c.get("http://captive.apple.com/hotspot-detect.html")
            .header("Cache-Control", "no-cache")
            .send()
            .await
        {
            if res.status().is_success() {
                if let Ok(text) = res.text().await {
                    if text.contains("Success") {
                        return true;
                    }
                }
            }
        }
    }
    false
}

async fn detect_login_type_rust() -> LoginType {
    let ips = [
        ("http://10.21.221.98/", LoginType::Type1),
        ("http://10.21.251.3/", LoginType::Type2),
        ("http://172.30.201.2/", LoginType::Type3),
        ("http://172.30.201.10/", LoginType::Type3),
    ];
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(1500))
        .build()
        .unwrap_or_default();
    for &(url, ref ltype) in &ips {
        if let Ok(res) = client.get(url)
            .header("Cache-Control", "no-cache")
            .send()
            .await
        {
            if res.status().as_u16() != 0 {
                return ltype.clone();
            }
        }
    }
    LoginType::Unknown
}

async fn login_to_campus_network_rust(
    login_type: LoginType,
    user: &str,
    pass: &str,
) -> Result<(bool, String), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(redact_request_error)?;
    match login_type {
        LoginType::Type1 => {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
            let v = format!("{:04}", nanos % 9000 + 1000);
            let user_encoded = url_encode(&(format!("{}@campus", user)));
            let pass_encoded = url_encode(pass);
            let url = format!(
                "http://10.21.221.98:801/eportal/portal/login?callback=dr1003&login_method=1&user_account={}&user_password={}&wlan_user_ip=&wlan_user_ipv6=&wlan_user_mac=000000000000&wlan_ac_ip=&wlan_ac_name=&jsVersion=4.2.1&terminal_type=1&lang=zh-cn&v={}",
                user_encoded, pass_encoded, v
            );
            let res = client.get(&url).send().await.map_err(redact_request_error)?;
            let text = res.text().await.map_err(redact_request_error)?;
            Ok(parse_dr_response(&text))
        }
        LoginType::Type2 => {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
            let v = format!("{:04}", nanos % 9000 + 1000);
            let user_encoded = url_encode(user);
            let pass_encoded = url_encode(pass);
            let url = format!(
                "http://10.21.251.3/drcom/login?callback=dr1002&DDDDD={}&upass={}&0MKKey=123456&R1=0&R2=&R3=0&R6=0&para=00&v6ip=&terminal_type=1&lang=zh-cn&jsVersion=4.1&v={}",
                user_encoded, pass_encoded, v
            );
            let res = client.get(&url).send().await.map_err(redact_request_error)?;
            let text = res.text().await.map_err(redact_request_error)?;
            Ok(parse_dr_response(&text))
        }
        LoginType::Type3 => {
            let mut params1 = std::collections::HashMap::new();
            params1.insert("DDDDD", user);
            params1.insert("upass", pass);
            params1.insert("v46s", "0");
            params1.insert("0MKKey", "");
            let res1 = client.post("https://lgn6.bjut.edu.cn/V6?https://lgn.bjut.edu.cn")
                .form(&params1)
                .send()
                .await
                .map_err(redact_request_error)?;
            let html = res1.text().await.map_err(redact_request_error)?;
            let v6ip = find_v6ip(&html);
            let mut params2 = std::collections::HashMap::new();
            params2.insert("DDDDD", user);
            params2.insert("upass", pass);
            params2.insert("0MKKey", "Login");
            params2.insert("v6ip", &v6ip);
            let res2 = client.post("https://lgn.bjut.edu.cn")
                .form(&params2)
                .send()
                .await
                .map_err(redact_request_error)?;
            let final_html = res2.text().await.map_err(redact_request_error)?;
            if final_html.contains("DispQianFei") || final_html.contains("Msg=") {
                Ok((false, "登录失败，请检查账号密码或余额".to_string()))
            } else {
                Ok((true, "Portal协议认证成功！".to_string()))
            }
        }
        _ => Err("未设定的登录类型".to_string()),
    }
}

fn redact_request_error(error: reqwest::Error) -> String {
    // Type 1/2 have to use the campus portal's GET protocol, which places
    // credentials in the query string. reqwest errors may otherwise include
    // that full URL and leak the password into app.log.
    error.without_url().to_string()
}

async fn fetch_user_info_rust(local_ip: Option<&str>) -> Option<UserInfo> {
    if let Some(ip) = local_ip {
        if !ip.starts_with("10.") && !ip.starts_with("172.") {
            return None;
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(1500))
        .build()
        .ok()?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let url = format!(
        "http://172.30.201.2:801/eportal/portal/page/loadUserInfo?callback=726427262624&lang=6c7e3b7578&program_index=79225954737327212323222f212e2723&page_index=755e577b7c4e27212323222f212e2320&user_account=&wlan_user_ip=&wlan_user_ipv6=&wlan_user_mac=262626262626262626262626&jsVersion=22384e&encrypt=1&v={:04}&lang=zh",
        nanos % 9000 + 1000
    );
    let text = client.get(url).send().await.ok()?.text().await.ok()?;
    let start = text.find('(')?;
    let end = text.rfind(')')?;
    let data: serde_json::Value = serde_json::from_str(&text[start + 1..end]).ok()?;
    let info = data.get("user_info")?;
    let package_name = info.get("package_group_name").and_then(|v| v.as_str()).unwrap_or("");
    let total_flow = if package_name.contains("Test") {
        None
    } else if package_name.contains("10元") {
        Some(60.0)
    } else if package_name.contains("20元") {
        Some(120.0)
    } else if package_name.contains("30元") {
        Some(180.0)
    } else if package_name.contains("60元") {
        Some(400.0)
    } else {
        Some(30.0)
    };
    let used_raw = info.get("use_flow").and_then(|v| v.as_str()).unwrap_or("0GB");
    let mut used = used_raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>()
        .parse::<f64>()
        .unwrap_or(0.0);
    if used_raw.contains("MB") {
        used /= 1024.0;
    }
    Some(UserInfo {
        account: info.get("account").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        balance: info.get("balance").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        flow: total_flow.map(|total| format!("{:.2} GB", total - used)).unwrap_or_else(|| "无限".to_string()),
    })
}

#[tauri::command]
fn get_update_target(app: tauri::AppHandle) -> UpdateTarget {
    let platform = if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let raw_arch = std::env::consts::ARCH;
    let arch = if platform == "android" && raw_arch == "x86_64" {
        "x86_64"
    } else if raw_arch == "aarch64" {
        "arm64"
    } else {
        "x64"
    };
    let format = match platform {
        "android" => "apk",
        "ios" => "unsupported",
        "windows" => "exe",
        "macos" => "dmg",
        _ if std::env::var_os("APPIMAGE").is_some() => "AppImage",
        _ => "deb",
    };
    UpdateTarget {
        platform: platform.to_string(),
        arch: arch.to_string(),
        format: format.to_string(),
        current_version: app.package_info().version.to_string(),
    }
}

#[cfg(target_os = "android")]
fn launch_update_installer(_app: &tauri::AppHandle, path: &std::path::Path) -> Result<(), String> {
    use jni::objects::{JObject, JValue};

    let context = tauri::tao::platform::android::prelude::main_android_context()
        .ok_or_else(|| "Android context is unavailable".to_string())?;
    let vm = unsafe { jni::JavaVM::from_raw(context.java_vm.cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread_as_daemon().map_err(|e| e.to_string())?;
    let activity = unsafe { JObject::from_raw(context.context_jobject.cast()) };
    let class = tauri::wry::prelude::find_class(
        &mut env,
        &activity,
        "cn.edu.bjut.al.UpdateHelper".into(),
    ).map_err(|e| e.to_string())?;
    let path_string = env.new_string(path.to_string_lossy().as_ref()).map_err(|e| e.to_string())?;
    let path_object = JObject::from(path_string);
    let launched = env.call_static_method(
        class,
        "installApk",
        "(Landroid/content/Context;Ljava/lang/String;)Z",
        &[JValue::Object(&activity), JValue::Object(&path_object)],
    ).map_err(|e| e.to_string())?.z().map_err(|e| e.to_string())?;
    if launched {
        Ok(())
    } else {
        Err("无法启动 APK 安装器，请允许此应用安装未知来源应用后重试".to_string())
    }
}

#[cfg(target_os = "windows")]
fn launch_update_installer(app: &tauri::AppHandle, path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new(path).spawn().map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

#[cfg(target_os = "macos")]
fn launch_update_installer(_app: &tauri::AppHandle, path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("open").arg(path).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn launch_update_installer(_app: &tauri::AppHandle, path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("xdg-open").arg(path).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(target_os = "ios")]
fn launch_update_installer(_app: &tauri::AppHandle, _path: &std::path::Path) -> Result<(), String> {
    Err("iOS 版本不支持应用内安装，请使用快捷指令更新".to_string())
}

#[tauri::command]
async fn download_and_install_update(
    app: tauri::AppHandle,
    url: String,
    file_name: String,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use std::io::Write;

    let parsed = reqwest::Url::parse(&url).map_err(|e| e.to_string())?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("github.com")
        || !parsed.path().starts_with("/key-zhzr/BJUT-Auto-Login/releases/download/")
    {
        return Err("拒绝下载非官方 GitHub Release 资产".to_string());
    }
    let safe_name = std::path::Path::new(&file_name)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "无效的更新文件名".to_string())?;
    if safe_name != file_name {
        return Err("无效的更新文件名".to_string());
    }

    let mut update_dir = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    update_dir.push("updates");
    std::fs::create_dir_all(&update_dir).map_err(|e| e.to_string())?;
    let target_path = update_dir.join(safe_name);

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(5))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client.get(parsed).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("更新下载失败: HTTP {}", response.status()));
    }
    let total = response.content_length();
    let mut received = 0u64;
    let mut file = std::fs::File::create(&target_path).map_err(|e| e.to_string())?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        received += chunk.len() as u64;
        let percent = total.map(|size| ((received as f64 / size as f64) * 100.0).min(100.0));
        let _ = app.emit("update-progress", serde_json::json!({
            "status": "downloading",
            "received": received,
            "total": total,
            "percent": percent
        }));
    }
    file.flush().map_err(|e| e.to_string())?;
    let _ = app.emit("update-progress", serde_json::json!({"status": "installing", "percent": 100.0}));
    launch_update_installer(&app, &target_path)
}

fn show_native_notification(app: &tauri::AppHandle, title: &str, body: &str) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| e.to_string())
}

fn get_config_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let mut p = app.path().app_config_dir().unwrap_or_else(|_| std::env::current_dir().unwrap());
    let _ = std::fs::create_dir_all(&p);
    p.push("config.json");
    p
}

fn get_log_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let mut p = app.path().app_data_dir().unwrap_or_else(|_| std::env::current_dir().unwrap());
    let _ = std::fs::create_dir_all(&p);
    p.push("app.log");
    p
}

fn get_account_health_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let mut path = app.path().app_data_dir().unwrap_or_else(|_| std::env::current_dir().unwrap());
    let _ = std::fs::create_dir_all(&path);
    path.push("account-health.json");
    path
}

fn load_account_health(app: &tauri::AppHandle) -> HashMap<String, AccountHealth> {
    std::fs::read_to_string(get_account_health_path(app))
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn persist_account_health(app: &tauri::AppHandle, health: &HashMap<String, AccountHealth>) {
    if let Ok(content) = serde_json::to_string_pretty(health) {
        let _ = std::fs::write(get_account_health_path(app), content);
    }
}

fn account_health_views(health: &HashMap<String, AccountHealth>) -> Vec<AccountHealthView> {
    let now = chrono::Utc::now().timestamp();
    let mut views: Vec<AccountHealthView> = health.iter().map(|(user, item)| {
        let cooldown_seconds = item.cooldown_until.map(|until| (until - now).max(0)).unwrap_or(0);
        let status = if cooldown_seconds > 0 && item.failure_kind.as_deref() == Some("credential") {
            "needs_attention"
        } else if cooldown_seconds > 0 {
            "cooling_down"
        } else if item.consecutive_failures > 0 {
            "degraded"
        } else {
            "healthy"
        };
        AccountHealthView {
            user: user.clone(),
            status: status.to_string(),
            consecutive_failures: item.consecutive_failures,
            cooldown_until: item.cooldown_until,
            cooldown_seconds,
            last_success: item.last_success.clone(),
            last_failure: item.last_failure.clone(),
            last_failure_reason: item.last_failure_reason.clone(),
            failure_kind: item.failure_kind.clone(),
        }
    }).collect();
    views.sort_by(|a, b| a.user.cmp(&b.user));
    views
}

fn current_account_health_views(state: &AppState) -> Vec<AccountHealthView> {
    let mut health = state.account_health.lock().unwrap().clone();
    let users: Vec<String> = state.config.read().unwrap().accounts
        .iter()
        .map(|account| account.user.clone())
        .collect();
    for user in users {
        health.entry(user).or_default();
    }
    account_health_views(&health)
}

fn classify_account_failure(reason: &str, consecutive_failures: u32) -> (&'static str, i64) {
    let normalized = reason.to_ascii_lowercase();
    if reason.contains("密码")
        || reason.contains("账号不存在")
        || reason.contains("用户名")
        || normalized.contains("password")
        || normalized.contains("credential")
    {
        return ("credential", 30 * 60);
    }
    if reason.contains("余额") || reason.contains("欠费") || normalized.contains("balance") {
        return ("balance", 6 * 60 * 60);
    }
    let exponent = consecutive_failures.saturating_sub(1).min(6);
    if reason.contains("请求出错")
        || reason.contains("超时")
        || normalized.contains("timeout")
        || normalized.contains("connect")
    {
        return ("network", (15_i64 * (1_i64 << exponent)).min(15 * 60));
    }
    ("server", (60_i64 * (1_i64 << exponent)).min(15 * 60))
}

fn account_attempt_allowed(state: &AppState, user: &str) -> Result<(), i64> {
    let now = chrono::Utc::now().timestamp();
    let health = state.account_health.lock().unwrap();
    let remaining = health.get(user)
        .and_then(|item| item.cooldown_until)
        .map(|until| (until - now).max(0))
        .unwrap_or(0);
    if remaining > 0 { Err(remaining) } else { Ok(()) }
}

fn emit_account_health(app: &tauri::AppHandle, state: &AppState) {
    let views = current_account_health_views(state);
    let _ = app.emit("account-health-change", views);
}

fn record_account_success(app: &tauri::AppHandle, state: &AppState, user: &str) {
    let snapshot = {
        let mut health = state.account_health.lock().unwrap();
        let item = health.entry(user.to_string()).or_default();
        item.consecutive_failures = 0;
        item.cooldown_until = None;
        item.last_success = Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
        item.last_failure_reason = None;
        item.failure_kind = None;
        health.clone()
    };
    persist_account_health(app, &snapshot);
    emit_account_health(app, state);
}

fn record_account_failure(app: &tauri::AppHandle, state: &AppState, user: &str, reason: &str) {
    let snapshot = {
        let mut health = state.account_health.lock().unwrap();
        let item = health.entry(user.to_string()).or_default();
        item.consecutive_failures = item.consecutive_failures.saturating_add(1);
        let (kind, cooldown_seconds) = classify_account_failure(reason, item.consecutive_failures);
        item.cooldown_until = Some(chrono::Utc::now().timestamp() + cooldown_seconds);
        item.last_failure = Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
        item.last_failure_reason = Some(reason.chars().take(200).collect());
        item.failure_kind = Some(kind.to_string());
        health.clone()
    };
    persist_account_health(app, &snapshot);
    emit_account_health(app, state);
}

fn reconcile_account_health_after_config_save(
    app: &tauri::AppHandle,
    state: &AppState,
    previous: &AppConfig,
    current: &AppConfig,
) {
    let snapshot = {
        let mut health = state.account_health.lock().unwrap();
        health.retain(|user, _| current.accounts.iter().any(|account| account.user == *user));
        for account in &current.accounts {
            let password_changed = previous.accounts.iter()
                .find(|candidate| candidate.user == account.user)
                .map(|candidate| candidate.pass != account.pass)
                .unwrap_or(true);
            if password_changed {
                health.remove(&account.user);
            }
        }
        health.clone()
    };
    persist_account_health(app, &snapshot);
    emit_account_health(app, state);
}

fn public_config(config: &AppConfig) -> AppConfig {
    let mut public = config.clone();
    for account in &mut public.accounts {
        account.pass.clear();
    }
    public
}

#[cfg(not(target_os = "android"))]
fn ensure_persistent_credential_backend() -> Result<(), String> {
    use keyring::credential::CredentialPersistence;

    match keyring::default::default_credential_builder().persistence() {
        CredentialPersistence::UntilDelete => Ok(()),
        _ => Err(
            "系统凭据库后端不是持久存储，已拒绝读写以避免重启后丢失密码".to_string()
        ),
    }
}

#[cfg(not(target_os = "android"))]
fn load_secure_config() -> Result<Option<AppConfig>, String> {
    ensure_persistent_credential_backend()?;
    let entry = keyring::Entry::new("cn.edu.bjut.al", "app-config").map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(serialized) => serde_json::from_str(&serialized).map(Some).map_err(|e| e.to_string()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(not(target_os = "android"))]
fn save_secure_config(config: &AppConfig) -> Result<(), String> {
    ensure_persistent_credential_backend()?;
    let entry = keyring::Entry::new("cn.edu.bjut.al", "app-config").map_err(|e| e.to_string())?;
    let serialized = serde_json::to_string(config).map_err(|e| e.to_string())?;
    entry.set_password(&serialized).map_err(|e| e.to_string())
}

fn save_secure_config_verified(config: &AppConfig) -> Result<(), String> {
    save_secure_config(config)?;
    match load_secure_config()? {
        Some(persisted) if persisted == *config => Ok(()),
        Some(_) => Err("安全存储回读内容与待保存配置不一致".to_string()),
        None => Err("安全存储写入后未能回读配置".to_string()),
    }
}

#[cfg(target_os = "android")]
fn android_secure_config(value: Option<&str>) -> Result<Option<String>, String> {
    use jni::objects::{JObject, JString, JValue};

    let context = tauri::tao::platform::android::prelude::main_android_context()
        .ok_or_else(|| "Android context is unavailable".to_string())?;
    let vm = unsafe { jni::JavaVM::from_raw(context.java_vm.cast()) }
        .map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread_as_daemon().map_err(|e| e.to_string())?;
    let activity = unsafe { JObject::from_raw(context.context_jobject.cast()) };
    let class = tauri::wry::prelude::find_class(
        &mut env,
        &activity,
        "cn.edu.bjut.al.NetworkHelper".into(),
    ).map_err(|e| e.to_string())?;

    if let Some(value) = value {
        let value = env.new_string(value).map_err(|e| e.to_string())?;
        let value_object = JObject::from(value);
        let saved = env.call_static_method(
            class,
            "setSecureConfig",
            "(Landroid/content/Context;Ljava/lang/String;)Z",
            &[JValue::Object(&activity), JValue::Object(&value_object)],
        );
        let saved = match saved {
            Ok(value) => value.z().map_err(|e| e.to_string())?,
            Err(error) => {
                let _ = env.exception_clear();
                return Err(format!("Android secure storage write failed: {error}"));
            }
        };
        return if saved { Ok(None) } else { Err("Android Keystore refused the configuration".to_string()) };
    }

    let result = env.call_static_method(
        class,
        "getSecureConfig",
        "(Landroid/content/Context;)Ljava/lang/String;",
        &[JValue::Object(&activity)],
    );
    let result = match result {
        Ok(value) => value.l().map_err(|e| e.to_string())?,
        Err(error) => {
            let _ = env.exception_clear();
            return Err(format!("Android secure storage read failed: {error}"));
        }
    };
    if result.is_null() {
        return Ok(None);
    }
    let value_string = JString::from(result);
    let value = env.get_string(&value_string).map_err(|e| e.to_string())?;
    let value: String = value.into();
    Ok((!value.is_empty()).then_some(value))
}

#[cfg(target_os = "android")]
fn load_secure_config() -> Result<Option<AppConfig>, String> {
    android_secure_config(None)?
        .map(|serialized| serde_json::from_str(&serialized).map_err(|e| e.to_string()))
        .transpose()
}

#[cfg(target_os = "android")]
fn save_secure_config(config: &AppConfig) -> Result<(), String> {
    let serialized = serde_json::to_string(config).map_err(|e| e.to_string())?;
    android_secure_config(Some(&serialized)).map(|_| ())
}

fn write_public_config(app: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    let content = serde_json::to_string_pretty(&public_config(config)).map_err(|e| e.to_string())?;
    std::fs::write(get_config_path(app), content).map_err(|e| e.to_string())
}

const LOG_SESSION_MARKER: &str = "=== SESSION START ===";
const MAX_LOG_SESSIONS: usize = 5;
const MAX_LOG_ENTRIES: usize = 2000;

fn parse_log_line(line: &str) -> Option<LogEntry> {
    let idx1 = line.find(']')?;
    line.strip_prefix('[')?;
    let time = line[1..idx1].to_string();
    let rest = &line[idx1 + 1..];
    let idx2 = rest.find('[')?;
    let idx3 = rest[idx2..].find(']')? + idx2;
    let log_type = rest[idx2 + 1..idx3].to_string();
    let rest2 = &rest[idx3 + 1..];
    let idx4 = rest2.find('[')?;
    let idx5 = rest2[idx4..].find(']')? + idx4;
    Some(LogEntry {
        time,
        module: rest2[idx4 + 1..idx5].to_string(),
        message: rest2[idx5 + 1..].trim().to_string(),
        log_type,
    })
}

fn initialize_log_history(app: &tauri::AppHandle, state: &AppState) {
    let path = get_log_path(app);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let existing_lines: Vec<&str> = existing.lines().collect();
    let session_starts: Vec<usize> = existing_lines.iter().enumerate()
        .filter_map(|(index, line)| line.contains(LOG_SESSION_MARKER).then_some(index))
        .collect();
    let keep_from = if session_starts.len() >= MAX_LOG_SESSIONS {
        session_starts[session_starts.len() - (MAX_LOG_SESSIONS - 1)]
    } else {
        0
    };
    let mut lines: Vec<String> = existing_lines[keep_from..].iter().map(|line| (*line).to_string()).collect();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    lines.push(format!("[{}] [info] [系统] {} {}", now, LOG_SESSION_MARKER, now));
    if lines.len() > MAX_LOG_ENTRIES {
        lines.drain(..lines.len() - MAX_LOG_ENTRIES);
    }
    let serialized = format!("{}\n", lines.join("\n"));
    let _ = std::fs::write(path, serialized);

    let mut memory = state.logs.lock().unwrap();
    memory.clear();
    memory.extend(lines.iter().filter_map(|line| parse_log_line(line)));
}

fn rust_log(app: &tauri::AppHandle, state: &AppState, module: &str, message: &str, log_type: &str) {
    let current_level = {
        let cfg = state.config.read().unwrap();
        cfg.log_level.clone()
    };
    if current_level == "error" && log_type != "error" {
        return;
    }
    if current_level == "info" && log_type == "debug" {
        return;
    }
    let local_now = chrono::Local::now();
    let time_str = local_now.format("%Y-%m-%d %H:%M:%S").to_string();
    let entry = LogEntry {
        time: time_str,
        module: module.to_string(),
        message: message.to_string(),
        log_type: log_type.to_string(),
    };
    {
        let mut logs = state.logs.lock().unwrap();
        logs.push(entry.clone());
        if logs.len() > MAX_LOG_ENTRIES {
            logs.remove(0);
        }
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(get_log_path(app))
    {
        use std::io::Write;
        let log_line = format!(
            "[{}] [{}] [{}] {}\n",
            entry.time, entry.log_type, entry.module, entry.message
        );
        let _ = file.write_all(log_line.as_bytes());
    }
    let _ = app.emit("log-event", entry);
}

fn merge_legacy_credentials(current: &mut AppConfig, legacy: &AppConfig) -> bool {
    let mut changed = false;
    for legacy_account in &legacy.accounts {
        if let Some(index) = current.accounts.iter()
            .position(|account| account.user == legacy_account.user)
        {
            let account = &mut current.accounts[index];
            if account.pass.is_empty() && !legacy_account.pass.is_empty() {
                account.pass = legacy_account.pass.clone();
                changed = true;
            }
        } else {
            let mut appended = legacy_account.clone();
            if current.accounts.iter().any(|account| account.is_default) {
                appended.is_default = false;
            }
            current.accounts.push(appended);
            changed = true;
        }
    }

    if !current.accounts.is_empty() && !current.accounts.iter().any(|account| account.is_default) {
        current.accounts[0].is_default = true;
        changed = true;
    }
    changed
}

fn fill_missing_passwords(target: &mut AppConfig, existing: &AppConfig) -> bool {
    let mut changed = false;
    for account in &mut target.accounts {
        if !account.pass.is_empty() {
            continue;
        }
        if let Some(saved) = existing.accounts.iter()
            .find(|candidate| candidate.user == account.user && !candidate.pass.is_empty())
        {
            account.pass = saved.pass.clone();
            changed = true;
        }
    }
    changed
}

fn load_config(app: &tauri::AppHandle, state: &AppState) {
    let p = get_config_path(app);
    let disk_config = std::fs::read_to_string(p)
        .ok()
        .and_then(|content| serde_json::from_str::<AppConfig>(&content).ok());

    match load_secure_config() {
        Ok(Some(mut config)) => {
            let migrated = disk_config.as_ref()
                .map(|legacy| merge_legacy_credentials(&mut config, legacy))
                .unwrap_or(false);
            let migration_persisted = if migrated {
                if let Err(error) = save_secure_config_verified(&config) {
                    eprintln!("Unable to migrate legacy credentials into secure storage: {error}");
                    false
                } else {
                    true
                }
            } else {
                true
            };
            // Do not erase the legacy plaintext passwords until the secure copy
            // has definitely been written; it may be the only recoverable copy.
            if migration_persisted && !migrated {
                let _ = write_public_config(app, &config);
            }
            *state.credential_storage_status.lock().unwrap() = if migration_persisted {
                "available".to_string()
            } else {
                "error".to_string()
            };
            *state.config.write().unwrap() = config;
        }
        Ok(None) => {
            if let Some(config) = disk_config {
                // Migrate legacy plaintext configurations once the system credential store is available.
                let mut status = "missing";
                if config.accounts.iter().any(|account| !account.pass.is_empty()) {
                    if let Err(error) = save_secure_config_verified(&config) {
                        eprintln!("Unable to migrate credentials to secure storage: {error}");
                        status = "error";
                    } else {
                        // Keep the legacy file for one complete restart. The
                        // Ok(Some) branch removes its passwords only after the
                        // system credential store returns the same data later.
                        status = "available";
                    }
                }
                *state.credential_storage_status.lock().unwrap() = status.to_string();
                *state.config.write().unwrap() = config;
            } else {
                *state.credential_storage_status.lock().unwrap() = "missing".to_string();
            }
        }
        Err(error) => {
            eprintln!("Unable to read secure credentials: {error}");
            *state.credential_storage_status.lock().unwrap() = "error".to_string();
            // Keep old installations usable even when the system credential store
            // is temporarily unavailable. A later explicit save still has to pass.
            if let Some(config) = disk_config {
                *state.config.write().unwrap() = config;
            }
        }
    }
}

fn save_config(app: &tauri::AppHandle, state: &AppState, mut new_cfg: AppConfig) -> Result<(), String> {
    let previous_cfg = {
        let state_cfg = state.config.read().unwrap();
        fill_missing_passwords(&mut new_cfg, &state_cfg);
        state_cfg.clone()
    };
    let storage_was_unreadable = {
        let status = state.credential_storage_status.lock().unwrap();
        status.as_str() == "error" || status.as_str() == "unknown"
    };
    if storage_was_unreadable && new_cfg.accounts.iter().any(|account| account.pass.is_empty()) {
        match load_secure_config() {
            Ok(Some(saved)) => {
                fill_missing_passwords(&mut new_cfg, &saved);
            }
            Ok(None) => {}
            Err(error) => {
                return Err(format!(
                    "安全存储暂时不可读，已阻止空密码覆盖原数据: {error}"
                ));
            }
        }
    }
    save_secure_config_verified(&new_cfg)?;
    *state.credential_storage_status.lock().unwrap() = "available".to_string();
    write_public_config(app, &new_cfg)?;
    {
        let mut state_cfg = state.config.write().unwrap();
        *state_cfg = new_cfg.clone();
    }
    reconcile_account_health_after_config_save(app, state, &previous_cfg, &new_cfg);
    Ok(())
}

async fn trigger_network_check(app: tauri::AppHandle, state: Arc<AppState>, full_details: bool) {
    if state.is_checking.swap(true, Ordering::SeqCst) {
        if full_details {
            state.pending_full_check.store(true, Ordering::SeqCst);
        }
        return;
    }
    if full_details {
        state.pending_full_check.store(false, Ordering::SeqCst);
    }
    tauri::async_runtime::spawn(async move {
        let is_bg = state.is_in_background.load(Ordering::SeqCst);
        let (interval_fg, interval_bg) = {
            let cfg = state.config.read().unwrap();
            (cfg.check_interval, cfg.check_interval_bg)
        };
        let next_interval = if is_bg { interval_bg } else { interval_fg };
        state.countdown.store(next_interval, Ordering::SeqCst);
        let _ = app.emit("countdown-tick", serde_json::json!({"status": "checking"}));
        rust_log(&app, &state, "网络", &format!("[DEBUG] 开始检测网络连通性 (模式: {})", if is_bg { "后台" } else { "前台" }), "debug");

        // Background interval checks reuse the last details and avoid location-protected APIs.
        let net_info = if full_details {
            get_network_info(app.clone())
        } else {
            state.last_network_state.lock().unwrap().clone()
        };
        let current_ssid = net_info.get("ssid").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let current_bssid = net_info.get("bssid").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let current_ip = net_info.get("ip").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let local_now = chrono::Local::now();
        let timestamp = local_now.format("%Y-%m-%d %H:%M:%S").to_string();

        if full_details {
            rust_log(&app, &state, "网络", &format!("[DEBUG] 完整检测网络详情: SSID={}, BSSID={}, IP={}", current_ssid, current_bssid, current_ip), "debug");
        } else {
            rust_log(&app, &state, "网络", "[DEBUG] 后台间隔检测仅检查连通性，复用上次网络详情", "debug");
        }

        let make_payload = |state_str: &str, login_type: Option<&LoginType>| {
            serde_json::json!({
                "state": state_str,
                "loginType": login_type.map(LoginType::as_str),
                "ssid": current_ssid.clone(),
                "bssid": current_bssid.clone(),
                "ip": current_ip.clone(),
                "timestamp": timestamp.clone()
            })
        };

        let is_online = check_internet_rust().await;
        rust_log(&app, &state, "网络", &format!("[DEBUG] 互联网可用性检测结果: {}", if is_online { "连通 (Online)" } else { "断开/受限" }), "debug");
        
        if is_online {
            rust_log(&app, &state, "网络", "网络检测完毕: 互联网已连通 (Online)", "info");
            state.is_checking.store(false, Ordering::SeqCst);
            state.non_campus_count.store(0, Ordering::SeqCst);
            let payload = make_payload("Online", None);
            {
                let mut last_state = state.last_network_state.lock().unwrap();
                *last_state = payload.clone();
            }
            let _ = app.emit("network-state-change", payload);
            return;
        }

        let login_type = detect_login_type_rust().await;
        rust_log(&app, &state, "网络", &format!("[DEBUG] 检测到校园网环境判定: {}", if login_type != LoginType::Unknown { "需要登录认证" } else { "非校园网/完全离线" }), "debug");
        
        match login_type {
            LoginType::Unknown => {
                state.non_campus_count.store(0, Ordering::SeqCst);
                rust_log(&app, &state, "网络", "网络检测完毕: 离线或非校园网 (Offline)", "info");
                let payload = make_payload("Offline", None);
                {
                    let mut last_state = state.last_network_state.lock().unwrap();
                    *last_state = payload.clone();
                }
                let _ = app.emit("network-state-change", payload);
            }
            _ => {
                rust_log(&app, &state, "网络", &format!("检测到校园网登录页面 (登录类型: {:?})", login_type), "info");
                let mut login_succeeded = false;
                let auto_login_enabled = {
                    let cfg = state.config.read().unwrap();
                    cfg.auto_login
                };
                if auto_login_enabled {
                    let (whitelist, blacklist) = {
                        let cfg = state.config.read().unwrap();
                        (cfg.whitelist.clone(), cfg.blacklist.clone())
                    };
                    let proceed = match automatic_login_network_allowed(
                        &login_type,
                        &current_ssid,
                        &current_bssid,
                        &current_ip,
                        &whitelist,
                        &blacklist,
                    ) {
                        Ok(()) => true,
                        Err(reason) => {
                            rust_log(
                                &app,
                                &state,
                                "安全",
                                &format!("自动登录已阻止: {reason}"),
                                "error",
                            );
                            false
                        }
                    };
                    if proceed {
                        let accounts = {
                            let cfg = state.config.read().unwrap();
                            cfg.accounts.clone()
                        };
                        let mut active_accounts = Vec::new();
                        for account in accounts.into_iter()
                            .filter(|acc| !acc.is_disabled.unwrap_or(false) && !acc.pass.is_empty())
                        {
                            match account_attempt_allowed(&state, &account.user) {
                                Ok(()) => active_accounts.push(account),
                                Err(remaining) => rust_log(
                                    &app,
                                    &state,
                                    "账号健康",
                                    &format!("账号 {} 仍在冷却中（剩余 {} 秒），跳过本次尝试", account.user, remaining),
                                    "info",
                                ),
                            }
                        }
                        if active_accounts.is_empty() {
                            rust_log(&app, &state, "网络", "未配置带已保存密码的有效账号，跳过自动登录", "error");
                        } else {
                            let mut success = false;
                            for acc in active_accounts {
                                rust_log(&app, &state, "网络", &format!("尝试使用账号 {} 自动登录...", acc.user), "info");
                                match login_to_campus_network_rust(login_type.clone(), &acc.user, &acc.pass).await {
                                    Ok((true, msg)) => {
                                        record_account_success(&app, &state, &acc.user);
                                        rust_log(&app, &state, "网络", &format!("登录成功: {}", msg), "success");
                                        let _ = show_native_notification(&app, "自动登录成功", &format!("账号: {}", acc.user));
                                        success = true;
                                        login_succeeded = true;
                                        break;
                                    }
                                    Ok((false, msg)) => {
                                        record_account_failure(&app, &state, &acc.user, &msg);
                                        rust_log(&app, &state, "网络", &format!("登录失败: {}", msg), "error");
                                    }
                                    Err(err) => {
                                        record_account_failure(&app, &state, &acc.user, &format!("请求出错: {err}"));
                                        rust_log(&app, &state, "网络", &format!("请求出错: {}", err), "error");
                                    }
                                }
                            }
                            if !success {
                                rust_log(&app, &state, "网络", "所有账号登录尝试完毕，均未成功", "error");
                            }
                        }
                    }
                } else {
                    rust_log(&app, &state, "网络", "自动登录未开启，忽略重连", "info");
                }
                if login_succeeded {
                    state.non_campus_count.store(0, Ordering::SeqCst);
                } else if is_bg {
                    let count = state.non_campus_count.fetch_add(1, Ordering::SeqCst) + 1;
                    rust_log(&app, &state, "网络", &format!("[DEBUG] 后台检测为非校园网环境，当前连续次数: {}/5", count), "debug");
                    if count >= 5 {
                        rust_log(&app, &state, "网络", "后台连续5次检测到校园网登录页面（或自动登录失败），进入自动休眠模式以省电。返回前台时将自动恢复。", "info");
                        state.is_suspended.store(true, Ordering::SeqCst);
                    }
                } else {
                    state.non_campus_count.store(0, Ordering::SeqCst);
                }
                let payload = if login_succeeded {
                    make_payload("Online", None)
                } else {
                    make_payload("BjutCampus", Some(&login_type))
                };
                {
                    let mut last_state = state.last_network_state.lock().unwrap();
                    *last_state = payload.clone();
                }
                let _ = app.emit("network-state-change", payload);
            }
        }
        state.is_checking.store(false, Ordering::SeqCst);
    });
}

#[tauri::command]
fn sync_config(app: tauri::AppHandle, state: tauri::State<Arc<AppState>>, config: AppConfig) -> Result<(), String> {
    if let Err(error) = save_config(&app, &state, config) {
        rust_log(&app, &state, "配置", &format!("配置持久化失败: {error}"), "error");
        return Err(error);
    }
    rust_log(&app, &state, "配置", "配置已写入安全存储", "debug");
    let is_bg = state.is_in_background.load(Ordering::SeqCst);
    let current_val = state.countdown.load(Ordering::SeqCst);
    let new_cfg = state.config.read().unwrap();
    let new_interval = if is_bg { new_cfg.check_interval_bg } else { new_cfg.check_interval };
    if current_val > new_interval {
        state.countdown.store(new_interval, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
fn get_app_config(state: tauri::State<Arc<AppState>>) -> AppConfig {
    state.config.read().unwrap().clone()
}

#[tauri::command]
fn get_credential_storage_status(state: tauri::State<Arc<AppState>>) -> String {
    state.credential_storage_status.lock().unwrap().clone()
}

fn credential_backend_name() -> &'static str {
    if cfg!(target_os = "android") {
        "Android Keystore (AES-GCM)"
    } else if cfg!(target_os = "macos") {
        "macOS Keychain"
    } else if cfg!(target_os = "windows") {
        "Windows Credential Manager"
    } else if cfg!(target_os = "linux") {
        "Linux Secret Service"
    } else {
        "系统安全存储"
    }
}

#[tauri::command]
fn get_credential_storage_health(state: tauri::State<Arc<AppState>>) -> CredentialStorageHealth {
    let status = state.credential_storage_status.lock().unwrap().clone();
    let config = state.config.read().unwrap();
    let missing_password_accounts: Vec<String> = config.accounts.iter()
        .filter(|account| account.pass.is_empty())
        .map(|account| account.user.clone())
        .collect();
    let saved_accounts = config.accounts.len().saturating_sub(missing_password_accounts.len());
    let message = match status.as_str() {
        "available" if missing_password_accounts.is_empty() => "系统安全存储工作正常，所有账号均已保存密码",
        "available" => "系统安全存储可用，但部分账号仍需补录密码",
        "missing" => "系统安全存储可用，当前尚未保存凭据",
        "error" => "系统安全存储暂时不可读；应用已阻止空密码覆盖",
        _ => "系统安全存储状态尚未完成初始化",
    }.to_string();
    CredentialStorageHealth {
        status,
        backend: credential_backend_name().to_string(),
        persistent: true,
        saved_accounts,
        missing_password_accounts,
        message,
    }
}

#[tauri::command]
fn get_account_health(state: tauri::State<Arc<AppState>>) -> Vec<AccountHealthView> {
    current_account_health_views(&state)
}

#[tauri::command]
fn reset_account_health(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    user: Option<String>,
) {
    let snapshot = {
        let mut health = state.account_health.lock().unwrap();
        if let Some(user) = user {
            health.remove(&user);
        } else {
            health.clear();
        }
        health.clone()
    };
    persist_account_health(&app, &snapshot);
    emit_account_health(&app, &state);
}

fn make_diagnostic_step(
    id: &str,
    label: &str,
    started: std::time::Instant,
    status: &str,
    message: String,
) -> DiagnosticStep {
    DiagnosticStep {
        id: id.to_string(),
        label: label.to_string(),
        status: status.to_string(),
        message,
        duration_ms: started.elapsed().as_millis(),
    }
}

#[tauri::command]
async fn run_network_diagnostics(app: tauri::AppHandle) -> DiagnosticReport {
    use std::net::ToSocketAddrs;

    let mut steps = Vec::new();
    let identity_started = std::time::Instant::now();
    let network = get_network_info(app);
    let ssid = network.get("ssid").and_then(|value| value.as_str()).unwrap_or("").to_string();
    let mut ip = network.get("ip").and_then(|value| value.as_str()).unwrap_or("").to_string();
    if ip.is_empty() {
        ip = get_local_ip();
    }
    let identity_status = if ip.is_empty() { "error" } else if ssid.is_empty() { "warning" } else { "success" };
    let identity_message = if ip.is_empty() {
        "未检测到可用网络接口或 IPv4 地址".to_string()
    } else if ssid.is_empty() || ssid.eq_ignore_ascii_case("<unknown ssid>") {
        format!("已取得本地 IP {ip}，但未取得无线网络名称（可能为有线网络或权限不足）")
    } else {
        format!("已连接 {ssid}，本地 IP {ip}")
    };
    steps.push(make_diagnostic_step(
        "network_identity",
        "网络接口",
        identity_started,
        identity_status,
        identity_message,
    ));

    let campus_started = std::time::Instant::now();
    let campus_ip = is_campus_local_ip(&ip);
    let campus_ssid = is_known_campus_ssid(&ssid);
    let campus_status = if campus_ip && (campus_ssid || ssid.is_empty() || ssid.contains("unknown")) {
        "success"
    } else if campus_ip || campus_ssid {
        "warning"
    } else {
        "error"
    };
    let campus_message = match (campus_ip, campus_ssid) {
        (true, true) => "SSID 与本地网段均符合校园网特征".to_string(),
        (true, false) => "本地网段符合校园网，但 SSID 未识别；自动登录需要白名单或有线协议".to_string(),
        (false, true) => "SSID 符合校园网，但本地 IP 不在已知网段".to_string(),
        (false, false) => "当前网络不符合已知校园网特征".to_string(),
    };
    steps.push(make_diagnostic_step(
        "campus_environment",
        "校园网环境",
        campus_started,
        campus_status,
        campus_message,
    ));

    let dns_started = std::time::Instant::now();
    let dns_ok = tauri::async_runtime::spawn_blocking(|| {
        ("www.baidu.com", 443).to_socket_addrs()
            .map(|mut addresses| addresses.next().is_some())
            .unwrap_or(false)
    }).await.unwrap_or(false);
    steps.push(make_diagnostic_step(
        "dns",
        "DNS 解析",
        dns_started,
        if dns_ok { "success" } else { "warning" },
        if dns_ok { "DNS 解析正常".to_string() } else { "DNS 解析失败或被认证页面限制".to_string() },
    ));

    let internet_started = std::time::Instant::now();
    let online = check_internet_rust().await;
    steps.push(make_diagnostic_step(
        "internet",
        "互联网连通性",
        internet_started,
        if online { "success" } else { "warning" },
        if online { "互联网访问正常，无需校园网认证".to_string() } else { "暂时无法访问互联网，继续检测认证网关".to_string() },
    ));

    let portal_started = std::time::Instant::now();
    let login_type = if online { LoginType::Unknown } else { detect_login_type_rust().await };
    let (portal_status, portal_message) = if online {
        ("success", "互联网已连通，跳过认证网关探测".to_string())
    } else if login_type != LoginType::Unknown {
        ("warning", format!("已检测到校园网认证协议 {}，当前需要登录", login_type.as_str()))
    } else {
        ("error", "未找到可访问的校园网认证网关，可能是完全离线或处于非校园网络".to_string())
    };
    steps.push(make_diagnostic_step(
        "portal",
        "认证网关",
        portal_started,
        portal_status,
        portal_message,
    ));

    let (overall, summary) = if online {
        ("healthy", "网络工作正常，互联网已连通")
    } else if login_type != LoginType::Unknown {
        ("auth_required", "已连接校园网，但需要完成账号认证")
    } else if ip.is_empty() {
        ("no_network", "未取得网络地址，请检查 Wi-Fi、有线连接或系统权限")
    } else {
        ("offline", "已取得本地网络，但无法访问互联网或校园网认证网关")
    };
    DiagnosticReport {
        created_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        overall: overall.to_string(),
        summary: summary.to_string(),
        ssid,
        ip,
        steps,
    }
}

#[tauri::command]
fn get_logs(state: tauri::State<Arc<AppState>>) -> Vec<LogEntry> {
    state.logs.lock().unwrap().clone()
}

#[tauri::command]
fn get_log_text(app: tauri::AppHandle) -> String {
    std::fs::read_to_string(get_log_path(&app)).unwrap_or_default()
}

#[tauri::command]
fn clear_all_logs(app: tauri::AppHandle, state: tauri::State<Arc<AppState>>) {
    state.logs.lock().unwrap().clear();
    let p = get_log_path(&app);
    let _ = std::fs::remove_file(p);
}

#[tauri::command]
fn get_countdown_status(state: tauri::State<Arc<AppState>>) -> serde_json::Value {
    let is_chk = state.is_checking.load(Ordering::SeqCst);
    let is_susp = state.is_suspended.load(Ordering::SeqCst);
    let current_countdown = state.countdown.load(Ordering::SeqCst);
    let status = if is_chk {
        "checking"
    } else if is_susp {
        "suspended"
    } else {
        "ticking"
    };
    serde_json::json!({
        "status": status,
        "seconds": current_countdown
    })
}

#[tauri::command]
fn trigger_manual_check(app: tauri::AppHandle, state: tauri::State<Arc<AppState>>) {
    rust_log(&app, &state, "网络", "收到手动连通性检测请求，开始检测...", "info");
    state.is_suspended.store(false, Ordering::SeqCst);
    state.non_campus_count.store(0, Ordering::SeqCst);
    let app_clone = app.clone();
    let state_clone = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        trigger_network_check(app_clone, state_clone, true).await;
    });
}

#[tauri::command]
async fn manual_login(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    account_index: Option<usize>,
    login_type_override: Option<String>,
) -> Result<ManualLoginResult, String> {
    let detected_type = detect_login_type_rust().await;
    let login_type = match login_type_override.as_deref() {
        Some("bjut-wifi") => LoginType::Type1,
        Some("bjut_sushe") => LoginType::Type2,
        Some("wired") => LoginType::Type3,
        _ => detected_type,
    };
    if login_type == LoginType::Unknown {
        return Ok(ManualLoginResult { success: false, message: "未检测到校园网登录页面".to_string() });
    }

    let configured_accounts = state.config.read().unwrap().accounts.clone();
    let accounts: Vec<Account> = match account_index {
        Some(index) => configured_accounts.get(index).cloned().into_iter().collect(),
        None => configured_accounts,
    }.into_iter().filter(|account| !account.is_disabled.unwrap_or(false)).collect();
    if accounts.is_empty() {
        return Ok(ManualLoginResult { success: false, message: "未配置可用账号".to_string() });
    }

    for account in accounts {
        if account.pass.is_empty() {
            rust_log(&app, &state, "登录", &format!("账号 {} 缺少已保存的密码", account.user), "error");
            continue;
        }
        if let Err(remaining) = account_attempt_allowed(&state, &account.user) {
            rust_log(
                &app,
                &state,
                "账号健康",
                &format!("账号 {} 正在冷却，剩余 {} 秒；可在网络诊断页解除", account.user, remaining),
                "info",
            );
            continue;
        }
        rust_log(&app, &state, "登录", &format!("尝试使用账号 {} 登录...", account.user), "info");
        match login_to_campus_network_rust(login_type.clone(), &account.user, &account.pass).await {
            Ok((true, message)) => {
                record_account_success(&app, &state, &account.user);
                rust_log(&app, &state, "登录", &format!("登录成功: {message}"), "success");
                let net_info = get_network_info(app.clone());
                let payload = serde_json::json!({
                    "state": "Online",
                    "ssid": net_info.get("ssid").and_then(|v| v.as_str()).unwrap_or(""),
                    "bssid": net_info.get("bssid").and_then(|v| v.as_str()).unwrap_or(""),
                    "ip": net_info.get("ip").and_then(|v| v.as_str()).unwrap_or(""),
                    "timestamp": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
                });
                *state.last_network_state.lock().unwrap() = payload.clone();
                let _ = app.emit("network-state-change", payload);
                return Ok(ManualLoginResult { success: true, message });
            }
            Ok((false, message)) => {
                record_account_failure(&app, &state, &account.user, &message);
                rust_log(&app, &state, "登录", &format!("登录失败: {message}"), "error");
            }
            Err(error) => {
                record_account_failure(&app, &state, &account.user, &format!("请求出错: {error}"));
                rust_log(&app, &state, "登录", &format!("请求出错: {error}"), "error");
            }
        }
    }
    Ok(ManualLoginResult { success: false, message: "所有可用账号均未能登录".to_string() })
}

#[tauri::command]
async fn get_user_info(local_ip: Option<String>) -> Option<UserInfo> {
    fetch_user_info_rust(local_ip.as_deref()).await
}

#[tauri::command]
fn log_from_js(app: tauri::AppHandle, state: tauri::State<Arc<AppState>>, module: String, message: String, log_type: String) {
    rust_log(&app, &state, &module, &message, &log_type);
}

#[tauri::command]
fn set_background_state(state: tauri::State<Arc<AppState>>, is_bg: bool) {
    state.is_in_background.store(is_bg, Ordering::SeqCst);
    let val = state.countdown.load(Ordering::SeqCst);
    let cfg = state.config.read().unwrap();
    let max_interval = if is_bg { cfg.check_interval_bg } else { cfg.check_interval };
    if val > max_interval {
        state.countdown.store(max_interval, Ordering::SeqCst);
    }
}

#[tauri::command]
fn get_current_network_state(state: tauri::State<Arc<AppState>>) -> serde_json::Value {
    state.last_network_state.lock().unwrap().clone()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .setup(|_app| {
            let app_state = std::sync::Arc::new(AppState {
                config: RwLock::new(AppConfig {
                    accounts: Vec::new(),
                    auto_login: false,
                    check_interval: 15,
                    check_interval_bg: 60,
                    wifi_change_detect: true,
                    log_level: "info".to_string(),
                    whitelist: Vec::new(),
                    blacklist: Vec::new(),
                }),
                credential_storage_status: Mutex::new("unknown".to_string()),
                account_health: Mutex::new(load_account_health(_app.handle())),
                logs: Mutex::new(Vec::new()),
                countdown: AtomicI32::new(15),
                is_checking: AtomicBool::new(false),
                pending_full_check: AtomicBool::new(false),
                is_suspended: AtomicBool::new(false),
                last_known_ip: Mutex::new(None),
                non_campus_count: AtomicU32::new(0),
                is_in_background: AtomicBool::new(false),
                last_network_state: Mutex::new(serde_json::json!({
                    "state": "Offline",
                    "ssid": "",
                    "bssid": "",
                    "ip": "",
                    "timestamp": "--"
                })),
            });
            _app.manage(app_state.clone());

            let app_handle = _app.handle().clone();
            let state_clone = app_state.clone();
            
            // Load config on startup
            load_config(&app_handle, &state_clone);
            
            initialize_log_history(&app_handle, &state_clone);

            // Start background loop task in Rust
            let loop_handle = app_handle.clone();
            let loop_state = state_clone.clone();
            tauri::async_runtime::spawn(async move {
                let mut wifi_check_counter = 0;
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    let is_bg = loop_state.is_in_background.load(Ordering::SeqCst);
                    let is_susp = loop_state.is_suspended.load(Ordering::SeqCst);
                    let is_chk = loop_state.is_checking.load(Ordering::SeqCst);

                    if !is_chk && loop_state.pending_full_check.swap(false, Ordering::SeqCst) {
                        rust_log(&loop_handle, &loop_state, "网络", "执行等待中的完整网络检测", "debug");
                        trigger_network_check(loop_handle.clone(), loop_state.clone(), true).await;
                        continue;
                    }

                    // 1. Wi-Fi Change Check Loop (every 3 seconds)
                    wifi_check_counter += 1;
                    if wifi_check_counter >= 3 {
                        wifi_check_counter = 0;
                        let wifi_change_detect = {
                            let cfg = loop_state.config.read().unwrap();
                            cfg.wifi_change_detect
                        };
                        if wifi_change_detect {
                            let (ip_changed, current_ip, last_ip) = {
                                let current_ip = get_local_ip();
                                let mut last_ip_lock = loop_state.last_known_ip.lock().unwrap();
                                let last_ip = last_ip_lock.clone();
                                let mut changed = false;
                                if !current_ip.is_empty() {
                                    if let Some(ref l_ip) = last_ip {
                                        if current_ip != *l_ip {
                                            changed = true;
                                        }
                                    }
                                    *last_ip_lock = Some(current_ip.clone());
                                } else if last_ip.is_some() {
                                    *last_ip_lock = None;
                                }
                                (changed, current_ip, last_ip)
                            };
                            rust_log(&loop_handle, &loop_state, "网络", &format!("[DEBUG] 执行 Wi-Fi 变更检测。当前 IP: {} (上次 IP: {})", current_ip, last_ip.as_ref().map(|s| s.as_str()).unwrap_or("空")), "debug");
                            if ip_changed {
                                rust_log(&loop_handle, &loop_state, "网络", &format!("检测到局域网 IP 发生变更: {} -> {}，重新检测网络环境...", last_ip.unwrap_or_default(), current_ip), "info");
                                loop_state.is_suspended.store(false, Ordering::SeqCst);
                                loop_state.non_campus_count.store(0, Ordering::SeqCst);
                                trigger_network_check(loop_handle.clone(), loop_state.clone(), true).await;
                                continue;
                            }
                        }
                    }

                    // 2. Connectivity Check Loop (every 1 second)
                    if !is_chk {
                        if !is_bg && is_susp {
                            rust_log(&loop_handle, &loop_state, "网络", "检测到已返回前台，恢复连通性检测...", "info");
                            loop_state.is_suspended.store(false, Ordering::SeqCst);
                            loop_state.non_campus_count.store(0, Ordering::SeqCst);
                            trigger_network_check(loop_handle.clone(), loop_state.clone(), true).await;
                            continue;
                        }
                        if is_susp {
                            let _ = loop_handle.emit("countdown-tick", serde_json::json!({"status": "suspended"}));
                            continue;
                        }
                        let val = loop_state.countdown.fetch_sub(1, Ordering::SeqCst);
                        let current_countdown = val - 1;
                        if current_countdown <= 0 {
                            rust_log(&loop_handle, &loop_state, "网络", "[DEBUG] 倒计时归零，触发自动网络连通性检测", "debug");
                            trigger_network_check(loop_handle.clone(), loop_state.clone(), !is_bg).await;
                        } else {
                            let _ = loop_handle.emit("countdown-tick", serde_json::json!({
                                "status": "ticking",
                                "seconds": current_countdown
                            }));
                        }
                    } else {
                        let _ = loop_handle.emit("countdown-tick", serde_json::json!({"status": "checking"}));
                    }
                }
            });

            let init_handle = app_handle.clone();
            let init_state = state_clone.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                trigger_network_check(init_handle, init_state, true).await;
            });

            #[cfg(desktop)]
            {
                use tauri::Manager;
                
                // Set frameless for non-macOS desktop windows
                #[cfg(not(target_os = "macos"))]
                {
                    if let Some(window) = _app.get_webview_window("main") {
                        let _ = window.set_decorations(false);
                    }
                }

                // Prevent window close, hide instead to keep in system tray
                if let Some(window) = _app.get_webview_window("main") {
                    let window_clone = window.clone();
                    let window_state = state_clone.clone();
                    window.on_window_event(move |event| {
                        match event {
                            tauri::WindowEvent::Focused(focused) => {
                                window_state.is_in_background.store(!focused, Ordering::SeqCst);
                            }
                            tauri::WindowEvent::CloseRequested { api, .. } => {
                                api.prevent_close();
                                window_state.is_in_background.store(true, Ordering::SeqCst);
                                let _ = window_clone.hide();
                            }
                            _ => {}
                        }
                    });
                }

                // System Tray Setup
                use tauri::menu::{Menu, MenuItem};
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

                let mut tray_builder = TrayIconBuilder::new();
                
                #[cfg(target_os = "macos")]
                {
                    let mac_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray_mac.png"))
                        .expect("Failed to load macOS tray icon");
                    tray_builder = tray_builder.icon(mac_icon);
                }
                #[cfg(not(target_os = "macos"))]
                {
                    if let Some(ic) = _app.default_window_icon().cloned() {
                        tray_builder = tray_builder.icon(ic);
                    }
                }

                // Create the system menu for right click / context menu
                let show_i = MenuItem::with_id(_app, "show", "显示主窗口", true, None::<&str>)?;
                let quit_i = MenuItem::with_id(_app, "quit", "退出", true, None::<&str>)?;
                let menu = Menu::with_items(_app, &[&show_i, &quit_i])?;

                let tray = tray_builder
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| {
                        if event.id == "show" {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        } else if event.id == "quit" {
                            app.exit(0);
                        }
                    })
                    .on_tray_icon_event(|tray_event, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            let app = tray_event.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                if window.is_visible().unwrap_or(false) {
                                    let _ = window.hide();
                                } else {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                            }
                        }
                    })
                    .build(_app)?;

                #[cfg(target_os = "macos")]
                {
                    let _ = tray.set_icon_as_template(true);
                }
            }
            Ok(())
        });

    #[allow(unused_mut)]
    let mut builder = builder
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init());

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_autostart::Builder::default().build());
    }

    let app = builder
        .invoke_handler(tauri::generate_handler![
            greet, 
            get_network_info, 
            request_battery_optimizations,
            request_foreground_permissions,
            request_background_permissions,
            start_keep_alive_service,
            stop_keep_alive_service,
            get_local_ip,
            exit_app,
            set_dock_visible,
            frontend_ready,
            read_clipboard,
            write_clipboard,
            sync_config,
            get_app_config,
            get_credential_storage_status,
            get_credential_storage_health,
            get_account_health,
            reset_account_health,
            run_network_diagnostics,
            get_logs,
            get_log_text,
            clear_all_logs,
            get_countdown_status,
            trigger_manual_check,
            manual_login,
            get_user_info,
            get_update_target,
            download_and_install_update,
            log_from_js,
            set_background_state,
            get_current_network_state
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        #[cfg(target_os = "macos")]
        {
            use tauri::Manager;
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        }
        let _ = app_handle;
        let _ = event;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_portal_query_values() {
        assert_eq!(url_encode("a b@北工大"), "a%20b%40%E5%8C%97%E5%B7%A5%E5%A4%A7");
    }

    #[test]
    fn parses_success_and_failure_portal_responses() {
        assert_eq!(parse_dr_response("dr1003({\"result\":1})"), (true, "Portal协议认证成功！".to_string()));
        assert_eq!(parse_dr_response("dr1002({\"result\":0,\"msga\":\"密码错误\"})"), (false, "密码错误".to_string()));
        assert_eq!(parse_dr_response("not json"), (false, "解析响应数据失败".to_string()));
    }

    #[test]
    fn extracts_v6ip_from_both_html_quote_styles() {
        assert_eq!(find_v6ip("<input name=\"v6ip\" value=\"2001:db8::1\">"), "2001:db8::1");
        assert_eq!(find_v6ip("<input name='v6ip' value='2001:db8::2'>"), "2001:db8::2");
    }

    #[test]
    fn public_config_never_contains_passwords() {
        let config = AppConfig {
            accounts: vec![Account { user: "20260001".to_string(), pass: "secret".to_string(), is_default: true, is_disabled: None }],
            auto_login: true,
            check_interval: 15,
            check_interval_bg: 60,
            wifi_change_detect: true,
            log_level: "info".to_string(),
            whitelist: vec![],
            blacklist: vec![],
        };
        let serialized = serde_json::to_string(&public_config(&config)).unwrap();
        assert!(!serialized.contains("secret"));
        assert_eq!(public_config(&config).accounts[0].pass, "");
    }

    #[test]
    fn migrates_passwords_from_legacy_plaintext_config() {
        let mut current: AppConfig = serde_json::from_str(r#"{
            "accounts":[{"user":"20260001","pass":"","isDefault":true}]
        }"#).unwrap();
        let legacy: AppConfig = serde_json::from_str(r#"{
            "accounts":[{"username":"20260001","password":"legacy-secret","is_default":true}],
            "autoLogin":true,
            "checkInterval":30
        }"#).unwrap();

        assert!(merge_legacy_credentials(&mut current, &legacy));
        assert_eq!(current.accounts[0].pass, "legacy-secret");
        assert!(legacy.auto_login);
        assert_eq!(legacy.check_interval, 30);
    }

    #[test]
    fn legacy_migration_never_overwrites_a_new_password() {
        let mut current: AppConfig = serde_json::from_str(r#"{
            "accounts":[{"user":"20260001","pass":"new-secret","isDefault":true}]
        }"#).unwrap();
        let legacy: AppConfig = serde_json::from_str(r#"{
            "accounts":[{"user":"20260001","pass":"old-secret","isDefault":true}]
        }"#).unwrap();

        assert!(!merge_legacy_credentials(&mut current, &legacy));
        assert_eq!(current.accounts[0].pass, "new-secret");
    }

    #[test]
    fn legacy_migration_keeps_current_values_and_adds_missing_accounts() {
        let mut current: AppConfig = serde_json::from_str(r#"{
            "accounts":[{"user":"a","pass":"new-a","isDefault":true}]
        }"#).unwrap();
        let legacy: AppConfig = serde_json::from_str(r#"{
            "accounts":[
                {"user":"a","pass":"old-a","isDefault":true},
                {"user":"b","pass":"old-b","isDefault":true}
            ]
        }"#).unwrap();

        assert!(merge_legacy_credentials(&mut current, &legacy));
        assert_eq!(current.accounts.len(), 2);
        assert_eq!(current.accounts[0].pass, "new-a");
        assert_eq!(current.accounts[1].user, "b");
        assert_eq!(current.accounts[1].pass, "old-b");
        assert!(!current.accounts[1].is_default);
    }

    #[test]
    fn blank_password_updates_reuse_existing_secrets() {
        let existing: AppConfig = serde_json::from_str(r#"{
            "accounts":[{"user":"a","pass":"saved-secret","isDefault":true}]
        }"#).unwrap();
        let mut update: AppConfig = serde_json::from_str(r#"{
            "accounts":[{"user":"a","pass":"","isDefault":true}]
        }"#).unwrap();

        assert!(fill_missing_passwords(&mut update, &existing));
        assert_eq!(update.accounts[0].pass, "saved-secret");
    }

    #[test]
    fn partial_password_repair_keeps_other_unrecoverable_accounts_editable() {
        let existing: AppConfig = serde_json::from_str(r#"{
            "accounts":[
                {"user":"a","pass":"","isDefault":true},
                {"user":"b","pass":"","isDefault":false}
            ]
        }"#).unwrap();
        let mut update = existing.clone();
        update.accounts[0].pass = "re-entered-secret".to_string();

        assert!(!fill_missing_passwords(&mut update, &existing));
        assert_eq!(update.accounts[0].pass, "re-entered-secret");
        assert!(update.accounts[1].pass.is_empty());
    }

    #[test]
    fn request_errors_never_include_credential_urls() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime.block_on(async {
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(100))
                .build()
                .unwrap()
                .get("http://127.0.0.1:0/login?user=student&password=top-secret")
                .send()
                .await
                .unwrap_err()
        });

        let message = redact_request_error(error);
        assert!(!message.contains("student"));
        assert!(!message.contains("top-secret"));
        assert!(!message.contains("password="));
    }

    #[test]
    fn automatic_login_requires_a_recognized_or_explicitly_trusted_network() {
        let whitelist = vec!["custom-campus|aa:bb".to_string()];
        assert!(!is_known_campus_ssid("evil-bjut-wifi"));
        assert!(automatic_login_network_allowed(
            &LoginType::Type1,
            "bjut_wifi",
            "11:22",
            "10.21.2.3",
            &[],
            &[],
        ).is_ok());
        assert!(automatic_login_network_allowed(
            &LoginType::Type1,
            "evil-ap",
            "11:22",
            "10.21.2.3",
            &[],
            &[],
        ).is_err());
        assert!(automatic_login_network_allowed(
            &LoginType::Type2,
            "custom-campus",
            "aa:bb",
            "10.21.2.3",
            &whitelist,
            &[],
        ).is_ok());
        assert!(automatic_login_network_allowed(
            &LoginType::Type3,
            "",
            "",
            "192.168.1.5",
            &[],
            &[],
        ).is_err());
    }

    #[test]
    fn account_failures_use_bounded_cooldowns() {
        assert_eq!(classify_account_failure("密码错误", 1), ("credential", 1800));
        assert_eq!(classify_account_failure("余额不足", 1), ("balance", 21600));
        assert_eq!(classify_account_failure("请求出错: timeout", 1), ("network", 15));
        assert_eq!(classify_account_failure("请求出错: timeout", 20), ("network", 900));
        assert_eq!(classify_account_failure("认证服务器繁忙", 1), ("server", 60));
        assert_eq!(classify_account_failure("认证服务器繁忙", 20), ("server", 900));
    }

    #[test]
    fn account_health_view_reports_active_cooldown() {
        let mut health = HashMap::new();
        health.insert("student".to_string(), AccountHealth {
            consecutive_failures: 1,
            cooldown_until: Some(chrono::Utc::now().timestamp() + 120),
            failure_kind: Some("credential".to_string()),
            ..Default::default()
        });

        let views = account_health_views(&health);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].status, "needs_attention");
        assert!(views[0].cooldown_seconds > 0);
    }
}
