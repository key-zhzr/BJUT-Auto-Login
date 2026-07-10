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
    }
    let _ = app;
    let _ = visible;
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

use std::sync::{Mutex, RwLock, Arc};
use std::sync::atomic::{AtomicI32, AtomicBool, AtomicU32, Ordering};
use tauri::Emitter;
use tauri::Manager;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct Account {
    user: String,
    pass: String,
    #[serde(rename = "isDefault")]
    is_default: bool,
    #[serde(rename = "isDisabled")]
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

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct AppConfig {
    #[serde(default)]
    accounts: Vec<Account>,
    #[serde(default = "default_auto_login")]
    auto_login: bool,
    #[serde(default = "default_check_interval")]
    check_interval: i32,
    #[serde(default = "default_check_interval_bg")]
    check_interval_bg: i32,
    #[serde(default = "default_wifi_change_detect")]
    wifi_change_detect: bool,
    #[serde(default = "default_log_level")]
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
    logs: Mutex<Vec<LogEntry>>,
    countdown: AtomicI32,
    is_checking: AtomicBool,
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
        .map_err(|e| e.to_string())?;
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
            let res = client.get(&url).send().await.map_err(|e| e.to_string())?;
            let text = res.text().await.map_err(|e| e.to_string())?;
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
            let res = client.get(&url).send().await.map_err(|e| e.to_string())?;
            let text = res.text().await.map_err(|e| e.to_string())?;
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
                .map_err(|e| e.to_string())?;
            let html = res1.text().await.map_err(|e| e.to_string())?;
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
                .map_err(|e| e.to_string())?;
            let final_html = res2.text().await.map_err(|e| e.to_string())?;
            if final_html.contains("DispQianFei") || final_html.contains("Msg=") {
                Ok((false, "登录失败，请检查账号密码或余额".to_string()))
            } else {
                Ok((true, "Portal协议认证成功！".to_string()))
            }
        }
        _ => Err("未设定的登录类型".to_string()),
    }
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

fn public_config(config: &AppConfig) -> AppConfig {
    let mut public = config.clone();
    for account in &mut public.accounts {
        account.pass.clear();
    }
    public
}

#[cfg(not(target_os = "android"))]
fn load_secure_config() -> Result<Option<AppConfig>, String> {
    let entry = keyring::Entry::new("cn.edu.bjut.al", "app-config").map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(serialized) => serde_json::from_str(&serialized).map(Some).map_err(|e| e.to_string()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(not(target_os = "android"))]
fn save_secure_config(config: &AppConfig) -> Result<(), String> {
    let entry = keyring::Entry::new("cn.edu.bjut.al", "app-config").map_err(|e| e.to_string())?;
    let serialized = serde_json::to_string(config).map_err(|e| e.to_string())?;
    entry.set_password(&serialized).map_err(|e| e.to_string())
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
    let class = env.find_class("cn/edu/bjut/al/NetworkHelper").map_err(|e| e.to_string())?;

    if let Some(value) = value {
        let value = env.new_string(value).map_err(|e| e.to_string())?;
        let value_object = JObject::from(value);
        let saved = env.call_static_method(
            class,
            "setSecureConfig",
            "(Landroid/content/Context;Ljava/lang/String;)Z",
            &[JValue::Object(&activity), JValue::Object(&value_object)],
        ).map_err(|e| e.to_string())?.z().map_err(|e| e.to_string())?;
        return if saved { Ok(None) } else { Err("Android Keystore refused the configuration".to_string()) };
    }

    let result = env.call_static_method(
        class,
        "getSecureConfig",
        "(Landroid/content/Context;)Ljava/lang/String;",
        &[JValue::Object(&activity)],
    ).map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?;
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

fn load_config(app: &tauri::AppHandle, state: &AppState) {
    let p = get_config_path(app);
    let disk_config = std::fs::read_to_string(p)
        .ok()
        .and_then(|content| serde_json::from_str::<AppConfig>(&content).ok());

    match load_secure_config() {
        Ok(Some(config)) => {
            let _ = write_public_config(app, &config);
            *state.config.write().unwrap() = config;
        }
        Ok(None) => {
            if let Some(config) = disk_config {
                // Migrate legacy plaintext configurations once the system credential store is available.
                if config.accounts.iter().any(|account| !account.pass.is_empty()) {
                    if let Err(error) = save_secure_config(&config).and_then(|_| write_public_config(app, &config)) {
                        eprintln!("Unable to migrate credentials to secure storage: {error}");
                    }
                }
                *state.config.write().unwrap() = config;
            }
        }
        Err(error) => eprintln!("Unable to read secure credentials: {error}"),
    }
}

fn save_config(app: &tauri::AppHandle, state: &AppState, new_cfg: AppConfig) -> Result<(), String> {
    save_secure_config(&new_cfg)?;
    write_public_config(app, &new_cfg)?;
    {
        let mut state_cfg = state.config.write().unwrap();
        *state_cfg = new_cfg.clone();
    }
    Ok(())
}

async fn trigger_network_check(app: tauri::AppHandle, state: Arc<AppState>, full_details: bool) {
    if state.is_checking.swap(true, Ordering::SeqCst) {
        return;
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
                    let mut proceed = true;
                    if !current_ssid.is_empty() {
                        let net_key = format!("{}|{}", current_ssid, current_bssid);
                        if blacklist.contains(&net_key) {
                            rust_log(&app, &state, "网络", &format!("当前 Wi-Fi ({}) 在黑名单中，跳过自动登录", current_ssid), "info");
                            proceed = false;
                        } else if !whitelist.is_empty() && !whitelist.contains(&net_key) {
                            rust_log(&app, &state, "网络", &format!("当前 Wi-Fi ({}) 不在白名单中，跳过自动登录", current_ssid), "info");
                            proceed = false;
                        }
                    }
                    if proceed {
                        let accounts = {
                            let cfg = state.config.read().unwrap();
                            cfg.accounts.clone()
                        };
                        let active_accounts: Vec<Account> = accounts.into_iter()
                            .filter(|acc| !acc.is_disabled.unwrap_or(false))
                            .collect();
                        if active_accounts.is_empty() {
                            rust_log(&app, &state, "网络", "未配置有效账号，跳过自动登录", "error");
                        } else {
                            let mut success = false;
                            for acc in active_accounts {
                                rust_log(&app, &state, "网络", &format!("尝试使用账号 {} 自动登录...", acc.user), "info");
                                match login_to_campus_network_rust(login_type.clone(), &acc.user, &acc.pass).await {
                                    Ok((true, msg)) => {
                                        rust_log(&app, &state, "网络", &format!("登录成功: {}", msg), "success");
                                        let _ = show_native_notification(&app, "自动登录成功", &format!("账号: {}", acc.user));
                                        success = true;
                                        login_succeeded = true;
                                        break;
                                    }
                                    Ok((false, msg)) => {
                                        rust_log(&app, &state, "网络", &format!("登录失败: {}", msg), "error");
                                    }
                                    Err(err) => {
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
                if is_bg {
                    let count = state.non_campus_count.fetch_add(1, Ordering::SeqCst) + 1;
                    rust_log(&app, &state, "网络", &format!("[DEBUG] 后台检测为非校园网环境，当前连续次数: {}/5", count), "debug");
                    if count >= 5 {
                        rust_log(&app, &state, "网络", "后台连续5次检测到校园网登录页面（或自动登录失败），进入自动休眠模式以省电。返回前台时将自动恢复。", "info");
                        state.is_suspended.store(true, Ordering::SeqCst);
                    }
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
    save_config(&app, &state, config)?;
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
    state.is_checking.store(false, Ordering::SeqCst);
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
        rust_log(&app, &state, "登录", &format!("尝试使用账号 {} 登录...", account.user), "info");
        match login_to_campus_network_rust(login_type.clone(), &account.user, &account.pass).await {
            Ok((true, message)) => {
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
            Ok((false, message)) => rust_log(&app, &state, "登录", &format!("登录失败: {message}"), "error"),
            Err(error) => rust_log(&app, &state, "登录", &format!("请求出错: {error}"), "error"),
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
                logs: Mutex::new(Vec::new()),
                countdown: AtomicI32::new(15),
                is_checking: AtomicBool::new(false),
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
            read_clipboard,
            write_clipboard,
            sync_config,
            get_app_config,
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
}
