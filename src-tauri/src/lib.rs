mod billing;
mod billing_runtime;
mod campus_services;
mod config_model;
mod cookie_jar;
mod recharge_state;
#[tauri::command]
fn get_network_info(
    _app: tauri::AppHandle,
    _include_wifi_details: Option<bool>,
) -> serde_json::Value {
    #[cfg(target_os = "android")]
    {
        let mut result = serde_json::json!({
            "ssid": "",
            "bssid": "",
            "ip": "",
            "transport": "unknown",
            "validated": false,
            "metered": false
        });
        if let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() {
            if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) } {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let activity =
                        unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };

                    match tauri::wry::prelude::find_class(
                        &mut env,
                        &activity,
                        "cn.edu.bjut.al.NetworkHelper".into(),
                    ) {
                        Ok(class) => {
                            let method_call = env.call_static_method(
                                class,
                                "getNetworkInfo",
                                "(Landroid/content/Context;Z)Ljava/lang/String;",
                                &[
                                    jni::objects::JValue::Object(&activity),
                                    jni::objects::JValue::Bool(
                                        if _include_wifi_details.unwrap_or(true) {
                                            1
                                        } else {
                                            0
                                        },
                                    ),
                                ],
                            );

                            match method_call {
                                Ok(jvalue) => {
                                    if let Ok(jobject) = jvalue.l() {
                                        let jstring: jni::objects::JString = jobject.into();
                                        if let Ok(rust_str) = env.get_string(&jstring).map(|s| {
                                            let s: String = s.into();
                                            s
                                        }) {
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
            if _include_wifi_details.unwrap_or(true) {
                if let Some(state) = _app.try_state::<Arc<AppState>>() {
                    rust_log(
                        &_app,
                        &state,
                        "隐私",
                        "[DEBUG] macOS 正在读取 SSID/BSSID；此操作可能显示系统位置使用指示",
                        "debug",
                    );
                }
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
            }

            if let Ok(output) = std::process::Command::new("sh")
                .arg("-c")
                .arg("ipconfig getifaddr en0")
                .output()
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let trimmed = stdout.trim();
                if !trimmed.is_empty() {
                    ip = trimmed.to_string();
                } else if let Ok(output2) = std::process::Command::new("sh")
                    .arg("-c")
                    .arg("ipconfig getifaddr en1")
                    .output()
                {
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
                    let activity =
                        unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };

                    let call =
                        env.call_method(&activity, "requestBatteryOptimizations", "()V", &[]);
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
                    let activity =
                        unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                    let _ = env.call_method(&activity, "requestForegroundPermissions", "()V", &[]);
                    if env.exception_check().unwrap_or(false) {
                        let _ = env.exception_clear();
                    }
                }
            }
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
                    let activity =
                        unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                    let _ = env.call_method(&activity, "requestBackgroundPermissions", "()V", &[]);
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
                    let activity =
                        unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                    let _ = env.call_method(&activity, "startKeepAliveService", "()V", &[]);
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
                    let activity =
                        unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                    let _ = env.call_method(&activity, "stopKeepAliveService", "()V", &[]);
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
        app.set_activation_policy(policy)
            .map_err(|e| e.to_string())?;
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
                        let activity =
                            unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                        if let Ok(class) = tauri::wry::prelude::find_class(
                            &mut env,
                            &activity,
                            "cn.edu.bjut.al.NetworkHelper".into(),
                        ) {
                            let method_call = env.call_static_method(
                                class,
                                "getLocalIpAddress",
                                "()Ljava/lang/String;",
                                &[],
                            );
                            if let Ok(jvalue) = method_call {
                                if let Ok(jobject) = jvalue.l() {
                                    let jstring: jni::objects::JString = jobject.into();
                                    if let Ok(rust_str) = env.get_string(&jstring).map(|s| {
                                        let s: String = s.into();
                                        s
                                    }) {
                                        ip = rust_str;
                                    }
                                }
                            }
                        }
                        if env.exception_check().unwrap_or(false) {
                            let _ = env.exception_clear();
                        }
                    }
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Ok(output) = std::process::Command::new("sh")
                .arg("-c")
                .arg("ipconfig getifaddr en0")
                .output()
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let trimmed = stdout.trim();
                if !trimmed.is_empty() {
                    ip = trimmed.to_string();
                } else if let Ok(output2) = std::process::Command::new("sh")
                    .arg("-c")
                    .arg("ipconfig getifaddr en1")
                    .output()
                {
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

use config_model::{
    default_android_notification_mode, default_balance_alert_threshold,
    default_flow_alert_threshold, default_vpn_compatibility, Account, AppConfig, NetworkProfile,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tauri::Emitter;
use tauri::Manager;

#[derive(serde::Serialize)]
struct ManualLoginResult {
    success: bool,
    message: String,
}

#[derive(serde::Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct UserInfo {
    account: String,
    balance: String,
    flow: String,
    source: String,
    status: Option<String>,
    status_reason: Option<String>,
    package: Option<String>,
    package_detail: Option<String>,
    used_flow: Option<String>,
    billing_cycle: Option<String>,
    updated_at: String,
    billing_error: Option<String>,
    login_history: Vec<billing::BillingLoginRecord>,
    online_sessions: Vec<billing::BillingOnlineSession>,
    offline_tip: Option<String>,
    mauth_enabled: Option<bool>,
    billing_warnings: Vec<String>,
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
    auto_login_paused_until: std::sync::atomic::AtomicI64,
    usage_alert_history: Mutex<HashMap<String, String>>,
    billing_fetch_lock: tokio::sync::Mutex<()>,
    campus_service_lock: tokio::sync::Mutex<()>,
    campus_recharge_pending: tokio::sync::Mutex<Option<campus_services::PendingRecharge>>,
    campus_alipay_recharge_pending:
        tokio::sync::Mutex<Option<campus_services::PendingAlipayRecharge>>,
    campus_wechat_recharge_pending:
        tokio::sync::Mutex<Option<campus_services::PendingWechatRecharge>>,
    campus_wechat_payment_pending:
        tokio::sync::Mutex<Option<campus_services::PendingWechatPayment>>,
    pending_discovered_account: tokio::sync::Mutex<Option<PendingDiscoveredAccount>>,
}

struct PendingDiscoveredAccount {
    account: Account,
    token: String,
    expires_at: std::time::Instant,
}

#[derive(Debug, Clone, PartialEq)]
enum LoginType {
    Type1, // 10.21.221.98 (eportal)
    Type2, // 10.21.251.3 (drcom)
    Type3, // lgn.bjut.edu.cn
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VpnCompatibility {
    /// HTTPS and the operating system resolver.
    Minimum,
    /// HTTPS, with the campus DNS server used to obtain reqwest overrides.
    Low,
    /// HTTPS, with known portal addresses pinned in reqwest while preserving SNI.
    High,
    /// Direct HTTP requests to the portal IP addresses.
    Maximum,
}

impl VpnCompatibility {
    fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "minimum" | "min" | "lowest" => Self::Minimum,
            "low" | "lower" => Self::Low,
            "maximum" | "max" | "highest" => Self::Maximum,
            _ => Self::High,
        }
    }

    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    fn as_str(self) -> &'static str {
        match self {
            Self::Minimum => "minimum",
            Self::Low => "low",
            Self::High => "high",
            Self::Maximum => "maximum",
        }
    }
}

fn effective_vpn_compatibility(config: &AppConfig) -> VpnCompatibility {
    let configured = VpnCompatibility::from_config(&config.vpn_compatibility);
    if configured == VpnCompatibility::Maximum
        && config
            .vpn_maximum_until
            .is_some_and(|until| until > chrono::Utc::now().timestamp())
    {
        VpnCompatibility::Maximum
    } else if configured == VpnCompatibility::Maximum {
        VpnCompatibility::High
    } else {
        configured
    }
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
    let octets: Vec<u8> = ip
        .split('.')
        .map(str::parse::<u8>)
        .collect::<Result<_, _>>()
        .unwrap_or_default();
    if octets.len() != 4 {
        return false;
    }
    matches!(
        (octets[0], octets[1]),
        (10, 17..=27) | (10, 121) | (10, 126) | (10, 226) | (172, 17..=27)
    )
}

fn is_known_campus_ssid(ssid: &str) -> bool {
    let normalized = ssid.trim().to_ascii_lowercase().replace('_', "-");
    normalized == "bjut-wifi" || normalized == "bjut-sushe"
}

const MOBILE_DATA_CHECK_INTERVAL_FOREGROUND: i32 = 120;
const MOBILE_DATA_CHECK_INTERVAL_BACKGROUND: i32 = 300;

fn network_transport(network: &serde_json::Value) -> &str {
    network
        .get("transport")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
}

fn is_mobile_data_network(network: &serde_json::Value) -> bool {
    #[cfg(target_os = "android")]
    {
        return network_transport(network).eq_ignore_ascii_case("cellular");
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = network;
        false
    }
}

fn network_is_system_validated(network: &serde_json::Value) -> bool {
    #[cfg(target_os = "android")]
    {
        return network
            .get("validated")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = network;
        false
    }
}

fn mobile_data_check_interval(configured: i32, is_background: bool) -> i32 {
    configured.max(if is_background {
        MOBILE_DATA_CHECK_INTERVAL_BACKGROUND
    } else {
        MOBILE_DATA_CHECK_INTERVAL_FOREGROUND
    })
}

fn automatic_login_network_allowed(
    login_type: &LoginType,
    ssid: &str,
    bssid: &str,
    ip: &str,
    transport: &str,
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
            if (transport.is_empty()
                || transport.eq_ignore_ascii_case("unknown")
                || transport.eq_ignore_ascii_case("ethernet"))
                && (normalized_ssid.is_empty()
                    || normalized_ssid.eq_ignore_ascii_case("unknown")
                    || normalized_ssid.eq_ignore_ascii_case("<unknown ssid>")) =>
        {
            Ok(())
        }
        LoginType::Type1 | LoginType::Type2 => {
            Err("无线网络名称未经识别，且未加入白名单".to_string())
        }
        LoginType::Type3 => Err("lgn 协议仅允许有线连接，当前网络未加入白名单".to_string()),
        LoginType::Unknown => Err("未识别到校园网认证协议".to_string()),
    }
}

fn login_type_from_profile(value: &str) -> Option<LoginType> {
    match value {
        "bjut-sushe" | "bjut_sushe" | "type1" => Some(LoginType::Type1),
        "bjut-wifi" | "bjut_wifi" | "type2" => Some(LoginType::Type2),
        "wired" | "type3" => Some(LoginType::Type3),
        _ => None,
    }
}

fn matching_network_profile(
    config: &AppConfig,
    ssid: &str,
    bssid: &str,
    detected_type: &LoginType,
) -> Option<NetworkProfile> {
    config
        .network_profiles
        .iter()
        .find(|profile| {
            if !profile.enabled {
                return false;
            }
            let ssid_matches = if profile.ssid.trim().is_empty() {
                *detected_type == LoginType::Type3
                    && (ssid.trim().is_empty()
                        || ssid.eq_ignore_ascii_case("unknown")
                        || ssid.eq_ignore_ascii_case("<unknown ssid>"))
            } else {
                profile.ssid.trim().eq_ignore_ascii_case(ssid.trim())
            };
            let bssid_matches = profile.bssid.trim().is_empty()
                || profile.bssid.trim().eq_ignore_ascii_case(bssid.trim());
            ssid_matches && bssid_matches
        })
        .cloned()
}

fn accounts_for_profile(accounts: Vec<Account>, profile: Option<&NetworkProfile>) -> Vec<Account> {
    let active: Vec<Account> = accounts
        .into_iter()
        .filter(|account| !account.is_disabled.unwrap_or(false) && !account.pass.is_empty())
        .collect();
    let Some(profile) = profile else {
        return active;
    };
    if profile.account_order.is_empty() {
        return active;
    }
    profile
        .account_order
        .iter()
        .filter_map(|user| active.iter().find(|account| account.user == *user).cloned())
        .collect()
}

fn profile_auto_login_enabled(
    profile: Option<&NetworkProfile>,
    login_type: &LoginType,
    global_default: bool,
) -> bool {
    let Some(profile) = profile else {
        return global_default;
    };
    let legacy_default = profile.auto_login.unwrap_or(global_default);
    let key = match login_type {
        LoginType::Type1 => "type1",
        LoginType::Type2 => "type2",
        LoginType::Type3 => "type3",
        LoginType::Unknown => return legacy_default,
    };
    profile
        .auto_login_types
        .get(key)
        .copied()
        .unwrap_or(legacy_default)
}

#[allow(unused_variables)]
fn app_is_in_background(app: &tauri::AppHandle, state: &AppState) -> bool {
    let reported_background = state.is_in_background.load(Ordering::SeqCst);
    #[cfg(desktop)]
    if let Some(window) = app.get_webview_window("main") {
        let visible = window.is_visible().unwrap_or(false);
        let focused = window.is_focused().unwrap_or(false);
        let minimized = window.is_minimized().unwrap_or(false);
        return reported_background || !visible || !focused || minimized;
    }
    reported_background
}

fn ensure_billing_foreground(state: &AppState) -> Result<(), String> {
    billing_runtime::ensure_foreground(&state.is_in_background)
}

async fn run_billing_read_while_foreground<T, F>(state: &AppState, future: F) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>>,
{
    billing_runtime::run_read_while_foreground(&state.is_in_background, future).await
}

async fn run_billing_mutation_to_completion<T, F>(state: &AppState, future: F) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>>,
{
    billing_runtime::run_mutation_to_completion(&state.is_in_background, future).await
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
                        let msg = data
                            .get("msg")
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
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(1800))
        .redirect(reqwest::redirect::Policy::none())
        .use_rustls_tls()
        .build()
    {
        Ok(client) => client,
        Err(_) => return false,
    };
    let targets = [
        ("https://connectivitycheck.gstatic.com/generate_204", 0u8),
        ("https://cp.cloudflare.com/generate_204", 0u8),
        ("http://captive.apple.com/hotspot-detect.html", 1u8),
        ("http://www.msftconnecttest.com/connecttest.txt", 2u8),
    ];
    let checks = targets.into_iter().map(|(url, validation)| {
        let client = client.clone();
        async move {
            let response = client
                .get(url)
                .header("Cache-Control", "no-cache, no-store")
                .send()
                .await
                .ok()?;
            match validation {
                0 => Some(response.status() == reqwest::StatusCode::NO_CONTENT),
                1 if response.status().is_success() => {
                    Some(response.text().await.ok()?.contains("Success"))
                }
                2 if response.status().is_success() => {
                    Some(response.text().await.ok()?.trim() == "Microsoft Connect Test")
                }
                _ => Some(false),
            }
        }
    });
    futures_util::future::join_all(checks)
        .await
        .into_iter()
        .flatten()
        .any(|online| online)
}

const CAMPUS_DNS_SERVER: &str = "10.21.200.28:53";
const WLGN_HOST: &str = "wlgn.bjut.edu.cn";
const LGN_HOST: &str = "lgn.bjut.edu.cn";
const LGN6_HOST: &str = "lgn6.bjut.edu.cn";

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

fn query_campus_dns_ipv4(host: &str) -> Result<Vec<std::net::Ipv4Addr>, String> {
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

    let socket = std::net::UdpSocket::bind("0.0.0.0:0")
        .map_err(|error| format!("无法创建校园网 DNS 查询：{error}"))?;
    let timeout = Some(std::time::Duration::from_millis(1200));
    socket
        .set_read_timeout(timeout)
        .map_err(|error| error.to_string())?;
    socket
        .set_write_timeout(timeout)
        .map_err(|error| error.to_string())?;
    socket
        .connect(CAMPUS_DNS_SERVER)
        .map_err(|error| format!("无法连接校园网 DNS：{error}"))?;
    socket
        .send(&query)
        .map_err(|error| format!("校园网 DNS 查询发送失败：{error}"))?;
    let mut packet = [0u8; 2048];
    let size = socket
        .recv(&mut packet)
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

async fn portal_client(
    compatibility: VpnCompatibility,
    login_type: &LoginType,
    timeout: std::time::Duration,
) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .use_rustls_tls();
    let hosts: Vec<(&str, Vec<std::net::Ipv4Addr>)> = match login_type {
        LoginType::Type2 => vec![(WLGN_HOST, vec![std::net::Ipv4Addr::new(10, 21, 251, 3)])],
        LoginType::Type3 => vec![
            (
                LGN_HOST,
                vec![
                    std::net::Ipv4Addr::new(172, 30, 201, 2),
                    std::net::Ipv4Addr::new(172, 30, 201, 10),
                ],
            ),
            (
                LGN6_HOST,
                vec![
                    std::net::Ipv4Addr::new(172, 30, 201, 2),
                    std::net::Ipv4Addr::new(172, 30, 201, 10),
                ],
            ),
        ],
        _ => Vec::new(),
    };
    if matches!(
        compatibility,
        VpnCompatibility::Low | VpnCompatibility::High
    ) {
        for (host, fixed_addresses) in hosts {
            let ipv4_addresses = if compatibility == VpnCompatibility::Low {
                let host_owned = host.to_string();
                tokio::task::spawn_blocking(move || query_campus_dns_ipv4(&host_owned))
                    .await
                    .map_err(|error| format!("校园网 DNS 任务失败：{error}"))??
            } else {
                fixed_addresses
            };
            let socket_addresses: Vec<std::net::SocketAddr> = ipv4_addresses
                .into_iter()
                .map(|address| std::net::SocketAddr::new(std::net::IpAddr::V4(address), 443))
                .collect();
            builder = builder.resolve_to_addrs(host, &socket_addresses);
        }
    }
    builder.build().map_err(redact_request_error)
}

fn portal_probe_urls(compatibility: VpnCompatibility, login_type: &LoginType) -> Vec<String> {
    match login_type {
        LoginType::Type1 if compatibility == VpnCompatibility::Maximum => {
            vec!["http://10.21.221.98:801/eportal/portal/login".to_string()]
        }
        LoginType::Type1 => vec!["https://10.21.221.98:802/eportal/portal/login".to_string()],
        LoginType::Type2 if compatibility == VpnCompatibility::Maximum => {
            vec!["http://10.21.251.3/drcom/login".to_string()]
        }
        LoginType::Type2 => vec!["https://wlgn.bjut.edu.cn/drcom/login".to_string()],
        LoginType::Type3 if compatibility == VpnCompatibility::Maximum => vec![
            "http://172.30.201.2".to_string(),
            "http://172.30.201.10".to_string(),
        ],
        LoginType::Type3 => vec!["https://lgn.bjut.edu.cn".to_string()],
        LoginType::Unknown => Vec::new(),
    }
}

async fn probe_login_type(
    compatibility: VpnCompatibility,
    login_type: LoginType,
) -> Option<LoginType> {
    let client = portal_client(
        compatibility,
        &login_type,
        std::time::Duration::from_millis(1800),
    )
    .await
    .ok()?;
    for url in portal_probe_urls(compatibility, &login_type) {
        if client
            .get(url)
            .header("Cache-Control", "no-cache")
            .send()
            .await
            .is_ok()
        {
            return Some(login_type);
        }
    }
    None
}

async fn detect_login_type_rust(compatibility: VpnCompatibility) -> LoginType {
    let probes = [LoginType::Type1, LoginType::Type2, LoginType::Type3]
        .into_iter()
        .map(|login_type| probe_login_type(compatibility, login_type));
    futures_util::future::join_all(probes)
        .await
        .into_iter()
        .flatten()
        .next()
        .unwrap_or(LoginType::Unknown)
}

async fn login_lgn_once(
    client: &reqwest::Client,
    first_url: &str,
    second_url: &str,
    user: &str,
    pass: &str,
) -> Result<(bool, String), String> {
    let mut first_form = std::collections::HashMap::new();
    first_form.insert("DDDDD", user);
    first_form.insert("upass", pass);
    first_form.insert("v46s", "0");
    first_form.insert("0MKKey", "");
    let first_response = client
        .post(first_url)
        .form(&first_form)
        .send()
        .await
        .map_err(redact_request_error)?;
    let html = first_response.text().await.map_err(redact_request_error)?;
    let v6ip = find_v6ip(&html);
    if v6ip.is_empty() {
        return Err("有线登录页未返回动态 IPv6 地址".to_string());
    }

    let mut second_form = std::collections::HashMap::new();
    second_form.insert("DDDDD", user);
    second_form.insert("upass", pass);
    second_form.insert("0MKKey", "Login");
    second_form.insert("v6ip", v6ip.as_str());
    let final_response = client
        .post(second_url)
        .form(&second_form)
        .send()
        .await
        .map_err(redact_request_error)?;
    let final_html = final_response.text().await.map_err(redact_request_error)?;
    if final_html.contains("DispQianFei") || final_html.contains("Msg=") {
        Ok((false, "登录失败，请检查账号密码或余额".to_string()))
    } else {
        Ok((true, "Portal协议认证成功！".to_string()))
    }
}

async fn login_to_campus_network_rust(
    login_type: LoginType,
    user: &str,
    pass: &str,
    compatibility: VpnCompatibility,
) -> Result<(bool, String), String> {
    let client = portal_client(
        compatibility,
        &login_type,
        std::time::Duration::from_secs(5),
    )
    .await?;
    match login_type {
        LoginType::Type1 => {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let v = format!("{:04}", nanos % 9000 + 1000);
            let user_encoded = url_encode(&(format!("{}@campus", user)));
            let pass_encoded = url_encode(pass);
            let base = if compatibility == VpnCompatibility::Maximum {
                "http://10.21.221.98:801/eportal/portal/login"
            } else {
                "https://10.21.221.98:802/eportal/portal/login"
            };
            let url = format!(
                "{base}?callback=dr1003&login_method=1&user_account={}&user_password={}&wlan_user_ip=&wlan_user_ipv6=&wlan_user_mac=000000000000&wlan_ac_ip=&wlan_ac_name=&jsVersion=4.2.1&terminal_type=1&lang=zh-cn&v={}",
                user_encoded, pass_encoded, v
            );
            let response = client
                .get(&url)
                .send()
                .await
                .map_err(redact_request_error)?;
            let text = response.text().await.map_err(redact_request_error)?;
            Ok(parse_dr_response(&text))
        }
        LoginType::Type2 => {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let v = format!("{:04}", nanos % 9000 + 1000);
            let user_encoded = url_encode(user);
            let pass_encoded = url_encode(pass);
            let base = if compatibility == VpnCompatibility::Maximum {
                "http://10.21.251.3/drcom/login"
            } else {
                "https://wlgn.bjut.edu.cn/drcom/login"
            };
            let url = format!(
                "{base}?callback=dr1002&DDDDD={}&upass={}&0MKKey=123456&R1=0&R2=&R3=0&R6=0&para=00&v6ip=&terminal_type=1&lang=zh-cn&jsVersion=4.1&v={}",
                user_encoded, pass_encoded, v
            );
            let response = client
                .get(&url)
                .send()
                .await
                .map_err(redact_request_error)?;
            let text = response.text().await.map_err(redact_request_error)?;
            Ok(parse_dr_response(&text))
        }
        LoginType::Type3 if compatibility == VpnCompatibility::Maximum => {
            let mut last_error = "有线登录网关不可达".to_string();
            for address in ["172.30.201.2", "172.30.201.10"] {
                let first_url = format!("http://{address}/V6?http://{address}");
                let second_url = format!("http://{address}");
                match login_lgn_once(&client, &first_url, &second_url, user, pass).await {
                    Ok(result) => return Ok(result),
                    Err(error) => last_error = error,
                }
            }
            Err(last_error)
        }
        LoginType::Type3 => {
            login_lgn_once(
                &client,
                "https://lgn6.bjut.edu.cn/V6?https://lgn.bjut.edu.cn",
                "https://lgn.bjut.edu.cn",
                user,
                pass,
            )
            .await
        }
        LoginType::Unknown => Err("未设定的登录类型".to_string()),
    }
}

#[cfg(target_os = "android")]
#[derive(serde::Serialize)]
struct HeadlessLog {
    module: String,
    message: String,
    #[serde(rename = "type")]
    log_type: String,
}

#[cfg(target_os = "android")]
fn headless_log(
    logs: &mut Vec<HeadlessLog>,
    module: &str,
    message: impl Into<String>,
    log_type: &str,
) {
    logs.push(HeadlessLog {
        module: module.to_string(),
        message: message.into(),
        log_type: log_type.to_string(),
    });
}

#[cfg(target_os = "android")]
async fn run_headless_network_check(
    config: AppConfig,
    network: serde_json::Value,
    reason: &str,
) -> serde_json::Value {
    let mut logs = Vec::new();
    let compatibility = effective_vpn_compatibility(&config);
    let transport = network_transport(&network).to_string();
    let validated = network
        .get("validated")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let ssid = network
        .get("ssid")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let bssid = network
        .get("bssid")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let ip = network
        .get("ip")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if config.log_level == "debug" {
        headless_log(
            &mut logs,
            "Android后台",
            format!(
                "[DEBUG] 无界面检测开始：来源={reason}，传输={transport}，系统验证={validated}，VPN兼容={}",
                compatibility.as_str()
            ),
            "debug",
        );
    }

    if transport.eq_ignore_ascii_case("cellular") {
        headless_log(
            &mut logs,
            "网络",
            "当前使用移动数据；已放缓后台检测并停止校园网网关探测",
            "info",
        );
        return serde_json::json!({
            "status": if validated { "online" } else { "cellular" },
            "notification_category": "network",
            "notification": if validated { "移动数据已连接，校园网探测已暂停" } else { "移动数据尚未通过系统验证" },
            "logs": logs,
        });
    }

    if validated || check_internet_rust().await {
        headless_log(&mut logs, "网络", "无界面检测完成：互联网已连通", "info");
        return serde_json::json!({
            "status": "online",
            "notification_category": "network",
            "notification": "后台检测正常，互联网已连接",
            "logs": logs,
        });
    }

    let detected = detect_login_type_rust(compatibility).await;
    let profile = matching_network_profile(&config, &ssid, &bssid, &detected);
    let login_type = profile
        .as_ref()
        .and_then(|item| login_type_from_profile(&item.login_type))
        .unwrap_or(detected);
    if login_type == LoginType::Unknown {
        headless_log(
            &mut logs,
            "网络",
            "无界面检测未找到可访问的校园网认证网关",
            "info",
        );
        return serde_json::json!({
            "status": "offline",
            "notification_category": "network",
            "notification": "网络离线或不在校园网环境",
            "logs": logs,
        });
    }

    if !profile_auto_login_enabled(profile.as_ref(), &login_type, config.auto_login) {
        headless_log(
            &mut logs,
            "网络",
            "已检测到校园网认证网关，但当前协议的自动登录已停用",
            "info",
        );
        return serde_json::json!({
            "status": "campus",
            "notification_category": "network",
            "notification": "校园网需要认证，自动登录已停用",
            "logs": logs,
        });
    }
    if let Err(reason) = automatic_login_network_allowed(
        &login_type,
        &ssid,
        &bssid,
        &ip,
        &transport,
        &config.whitelist,
        &config.blacklist,
    ) {
        headless_log(
            &mut logs,
            "安全",
            format!("无界面自动登录已阻止：{reason}"),
            "error",
        );
        return serde_json::json!({
            "status": "blocked",
            "notification_category": "login",
            "notification": "校园网登录被安全策略阻止",
            "logs": logs,
        });
    }

    let accounts = accounts_for_profile(config.accounts.clone(), profile.as_ref());
    if accounts.is_empty() {
        headless_log(
            &mut logs,
            "网络",
            "没有可供无界面自动登录使用的已保存账号",
            "error",
        );
        return serde_json::json!({
            "status": "campus",
            "notification_category": "login",
            "notification": "校园网需要认证，但没有可用账号",
            "logs": logs,
        });
    }

    let mut last_message = "所有账号均登录失败".to_string();
    for account in accounts {
        headless_log(
            &mut logs,
            "网络",
            format!("无界面核心尝试使用账号 {} 自动登录", account.user),
            "info",
        );
        match login_to_campus_network_rust(
            login_type.clone(),
            &account.user,
            &account.pass,
            compatibility,
        )
        .await
        {
            Ok((true, message)) => {
                headless_log(
                    &mut logs,
                    "网络",
                    format!("无界面自动登录成功：账号 {}，{message}", account.user),
                    "success",
                );
                return serde_json::json!({
                    "status": "login_success",
                    "notification_category": "login",
                    "notification": format!("校园网自动登录成功：{}", account.user),
                    "logs": logs,
                });
            }
            Ok((false, message)) => {
                last_message = message.clone();
                headless_log(
                    &mut logs,
                    "网络",
                    format!("无界面自动登录失败：账号 {}，{message}", account.user),
                    "error",
                );
            }
            Err(error) => {
                last_message = error.clone();
                headless_log(
                    &mut logs,
                    "网络",
                    format!("无界面登录请求失败：账号 {}，{error}", account.user),
                    "error",
                );
            }
        }
    }
    serde_json::json!({
        "status": "login_failed",
        "notification_category": "login",
        "notification": format!("校园网自动登录失败：{last_message}"),
        "logs": logs,
    })
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_cn_edu_bjut_al_NativeKeepAlive_runHeadlessCheck(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    config_json: jni::objects::JString,
    network_info_json: jni::objects::JString,
    reason: jni::objects::JString,
) -> jni::sys::jstring {
    let result = (|| -> Result<String, String> {
        let config_json: String = env
            .get_string(&config_json)
            .map_err(|error| error.to_string())?
            .into();
        let network_info_json: String = env
            .get_string(&network_info_json)
            .map_err(|error| error.to_string())?
            .into();
        let reason: String = env
            .get_string(&reason)
            .map_err(|error| error.to_string())?
            .into();
        let config: AppConfig = serde_json::from_str(&config_json)
            .map_err(|error| format!("解析安全配置失败：{error}"))?;
        let network: serde_json::Value = serde_json::from_str(&network_info_json)
            .map_err(|error| format!("解析网络信息失败：{error}"))?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("创建无界面运行时失败：{error}"))?;
        let payload = runtime.block_on(run_headless_network_check(config, network, &reason));
        serde_json::to_string(&payload).map_err(|error| error.to_string())
    })()
    .unwrap_or_else(|error| {
        serde_json::json!({
            "status": "error",
            "notification_category": "background",
            "notification": "后台检测核心启动失败",
            "logs": [{
                "module": "Android后台",
                "message": error,
                "type": "error"
            }]
        })
        .to_string()
    });
    env.new_string(result)
        .map(|value| value.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

fn redact_request_error(error: reqwest::Error) -> String {
    // Type 1/2 have to use the campus portal's GET protocol, which places
    // credentials in the query string. reqwest errors may otherwise include
    // that full URL and leak the password into app.log.
    error.without_url().to_string()
}

fn lgn_user_info_url(random: u16) -> String {
    format!(
        "https://{LGN_HOST}:802/eportal/portal/page/loadUserInfo?callback=726427262624&lang=6c7e3b7578&program_index=79225954737327212323222f212e2723&page_index=755e577b7c4e27212323222f212e2320&user_account=&wlan_user_ip=&wlan_user_ipv6=&wlan_user_mac=262626262626262626262626&jsVersion=22384e&encrypt=1&v={random:04}&lang=zh"
    )
}

async fn fetch_portal_user_info(
    local_ip: Option<&str>,
    compatibility: VpnCompatibility,
) -> Option<UserInfo> {
    if let Some(ip) = local_ip {
        if !ip.starts_with("10.") && !ip.starts_with("172.") {
            return None;
        }
    }

    let client = portal_client(
        compatibility,
        &LoginType::Type3,
        std::time::Duration::from_secs(3),
    )
    .await
    .ok()?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let url = lgn_user_info_url((nanos % 9000 + 1000) as u16);
    let text = client.get(url).send().await.ok()?.text().await.ok()?;
    let start = text.find('(')?;
    let end = text.rfind(')')?;
    let data: serde_json::Value = serde_json::from_str(&text[start + 1..end]).ok()?;
    let info = data.get("user_info")?;
    let package_name = info
        .get("package_group_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
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
    let used_raw = info
        .get("use_flow")
        .and_then(|v| v.as_str())
        .unwrap_or("0GB");
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
        account: info
            .get("account")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        balance: info
            .get("balance")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        flow: total_flow
            .map(|total| format!("{:.2} GB", total - used))
            .unwrap_or_else(|| "无限".to_string()),
        source: "portal".to_string(),
        status: None,
        status_reason: None,
        package: (!package_name.is_empty()).then(|| package_name.to_string()),
        package_detail: None,
        used_flow: Some(used_raw.to_string()),
        billing_cycle: None,
        updated_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        billing_error: None,
        login_history: Vec::new(),
        online_sessions: Vec::new(),
        offline_tip: None,
        mauth_enabled: None,
        billing_warnings: Vec::new(),
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
    let mut env = vm
        .attach_current_thread_as_daemon()
        .map_err(|e| e.to_string())?;
    let activity = unsafe { JObject::from_raw(context.context_jobject.cast()) };
    let class =
        tauri::wry::prelude::find_class(&mut env, &activity, "cn.edu.bjut.al.UpdateHelper".into())
            .map_err(|e| e.to_string())?;
    let path_string = env
        .new_string(path.to_string_lossy().as_ref())
        .map_err(|e| e.to_string())?;
    let path_object = JObject::from(path_string);
    let launched = env
        .call_static_method(
            class,
            "installApk",
            "(Landroid/content/Context;Ljava/lang/String;)Z",
            &[JValue::Object(&activity), JValue::Object(&path_object)],
        )
        .map_err(|e| e.to_string())?
        .z()
        .map_err(|e| e.to_string())?;
    if launched {
        Ok(())
    } else {
        Err("无法启动 APK 安装器，请允许此应用安装未知来源应用后重试".to_string())
    }
}

#[cfg(target_os = "ios")]
fn launch_update_installer(_app: &tauri::AppHandle, _path: &std::path::Path) -> Result<(), String> {
    Err("iOS 版本不支持应用内安装，请使用快捷指令更新".to_string())
}

#[cfg(any(target_os = "android", target_os = "ios"))]
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
        || !parsed
            .path()
            .starts_with("/key-zhzr/BJUT-Auto-Login/releases/download/")
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
        .use_rustls_tls()
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
        let _ = app.emit(
            "update-progress",
            serde_json::json!({
                "status": "downloading",
                "received": received,
                "total": total,
                "percent": percent
            }),
        );
    }
    file.flush().map_err(|e| e.to_string())?;
    let _ = app.emit(
        "update-progress",
        serde_json::json!({"status": "installing", "percent": 100.0}),
    );
    launch_update_installer(&app, &target_path)
}

#[cfg(desktop)]
#[tauri::command]
async fn download_and_install_update(
    app: tauri::AppHandle,
    url: String,
    _file_name: String,
) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;

    let endpoint = reqwest::Url::parse(&url).map_err(|error| error.to_string())?;
    if endpoint.scheme() != "https"
        || endpoint.host_str() != Some("github.com")
        || !endpoint
            .path()
            .starts_with("/key-zhzr/BJUT-Auto-Login/releases/download/")
        || !endpoint.path().ends_with("/latest.json")
    {
        return Err("拒绝使用非官方签名更新清单".to_string());
    }
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| error.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "签名更新清单未提供适用于当前设备的新版本".to_string())?;
    let mut received = 0u64;
    update
        .download_and_install(
            |chunk_length, content_length| {
                received += chunk_length as u64;
                let percent = content_length
                    .map(|total| ((received as f64 / total as f64) * 100.0).min(100.0));
                let _ = app.emit(
                    "update-progress",
                    serde_json::json!({
                        "status": "downloading",
                        "received": received,
                        "total": content_length,
                        "percent": percent,
                    }),
                );
            },
            || {
                let _ = app.emit(
                    "update-progress",
                    serde_json::json!({
                        "status": "installing",
                        "percent": 100.0,
                    }),
                );
            },
        )
        .await
        .map_err(|error| format!("更新签名验证或安装失败：{error}"))?;
    app.restart();
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

fn automatic_login_result_notifications_enabled(state: &AppState) -> bool {
    #[cfg(target_os = "android")]
    {
        state.config.read().unwrap().android_notify_login_results
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = state;
        true
    }
}

fn get_config_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let mut p = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap());
    let _ = std::fs::create_dir_all(&p);
    p.push("config.json");
    p
}

fn get_log_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let mut p = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap());
    let _ = std::fs::create_dir_all(&p);
    p.push("app.log");
    p
}

fn get_account_health_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let mut path = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap());
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
    let mut views: Vec<AccountHealthView> = health
        .iter()
        .map(|(user, item)| {
            let cooldown_seconds = item
                .cooldown_until
                .map(|until| (until - now).max(0))
                .unwrap_or(0);
            let status =
                if cooldown_seconds > 0 && item.failure_kind.as_deref() == Some("credential") {
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
        })
        .collect();
    views.sort_by(|a, b| a.user.cmp(&b.user));
    views
}

fn current_account_health_views(state: &AppState) -> Vec<AccountHealthView> {
    let mut health = state.account_health.lock().unwrap().clone();
    let users: Vec<String> = state
        .config
        .read()
        .unwrap()
        .accounts
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
    let remaining = health
        .get(user)
        .and_then(|item| item.cooldown_until)
        .map(|until| (until - now).max(0))
        .unwrap_or(0);
    if remaining > 0 {
        Err(remaining)
    } else {
        Ok(())
    }
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
            let password_changed = previous
                .accounts
                .iter()
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
    // Network trust rules and payment recovery metadata are stored only in the
    // encrypted credential backend, never in config.json.
    public.whitelist.clear();
    public.blacklist.clear();
    public.campus_service_sessions.clear();
    public.recharge_transactions = recharge_state::RechargeJournal::default();
    public
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn ensure_persistent_credential_backend() -> Result<(), String> {
    use keyring::credential::CredentialPersistence;

    match keyring::default::default_credential_builder().persistence() {
        CredentialPersistence::UntilDelete => Ok(()),
        _ => Err("系统凭据库后端不是持久存储，已拒绝读写以避免重启后丢失密码".to_string()),
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn load_secure_config(_app: &tauri::AppHandle) -> Result<Option<AppConfig>, String> {
    ensure_persistent_credential_backend()?;
    let entry = keyring::Entry::new("cn.edu.bjut.al", "app-config").map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(serialized) => serde_json::from_str(&serialized)
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn save_secure_config(_app: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    ensure_persistent_credential_backend()?;
    let entry = keyring::Entry::new("cn.edu.bjut.al", "app-config").map_err(|e| e.to_string())?;
    let serialized = serde_json::to_string(config).map_err(|e| e.to_string())?;
    entry.set_password(&serialized).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn macos_credential_paths(
    app: &tauri::AppHandle,
) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    use std::os::unix::fs::PermissionsExt;

    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    Ok((
        directory.join("credentials.key"),
        directory.join("credentials.enc"),
    ))
}

#[cfg(target_os = "macos")]
fn load_or_create_macos_credential_key(app: &tauri::AppHandle) -> Result<[u8; 32], String> {
    use aes_gcm::aead::{rand_core::RngCore, OsRng};
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let (key_path, _) = macos_credential_paths(app)?;
    if let Ok(bytes) = std::fs::read(&key_path) {
        return bytes
            .try_into()
            .map_err(|_| "macOS 本地凭据密钥长度无效".to_string());
    }
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&key_path)
        .map_err(|error| error.to_string())?;
    file.write_all(&key).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    Ok(key)
}

#[cfg(target_os = "macos")]
fn load_secure_config(app: &tauri::AppHandle) -> Result<Option<AppConfig>, String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};

    let (_, encrypted_path) = macos_credential_paths(app)?;
    let payload = match std::fs::read(encrypted_path) {
        Ok(payload) => payload,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if payload.len() < 13 || payload[0] != 1 {
        return Err("macOS 本地凭据文件格式无效".to_string());
    }
    let key = load_or_create_macos_credential_key(app)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|error| error.to_string())?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&payload[1..13]), &payload[13..])
        .map_err(|_| "macOS 本地凭据文件认证失败".to_string())?;
    serde_json::from_slice(&plaintext)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn save_secure_config(app: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    use aes_gcm::{
        aead::{rand_core::RngCore, Aead, OsRng},
        Aes256Gcm, KeyInit, Nonce,
    };
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let key = load_or_create_macos_credential_key(app)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|error| error.to_string())?;
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let serialized = serde_json::to_vec(config).map_err(|error| error.to_string())?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), serialized.as_ref())
        .map_err(|_| "macOS 本地凭据加密失败".to_string())?;
    let (_, encrypted_path) = macos_credential_paths(app)?;
    let temporary_path = encrypted_path.with_extension("enc.tmp");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary_path)
        .map_err(|error| error.to_string())?;
    file.write_all(&[1])
        .and_then(|_| file.write_all(&nonce))
        .and_then(|_| file.write_all(&ciphertext))
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(temporary_path, encrypted_path).map_err(|error| error.to_string())
}

fn save_secure_config_verified(app: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    save_secure_config(app, config)?;
    match load_secure_config(app)? {
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
    let vm = unsafe { jni::JavaVM::from_raw(context.java_vm.cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm
        .attach_current_thread_as_daemon()
        .map_err(|e| e.to_string())?;
    let activity = unsafe { JObject::from_raw(context.context_jobject.cast()) };
    let class =
        tauri::wry::prelude::find_class(&mut env, &activity, "cn.edu.bjut.al.NetworkHelper".into())
            .map_err(|e| e.to_string())?;

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
        return if saved {
            Ok(None)
        } else {
            Err("Android Keystore refused the configuration".to_string())
        };
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
fn load_secure_config(_app: &tauri::AppHandle) -> Result<Option<AppConfig>, String> {
    android_secure_config(None)?
        .map(|serialized| serde_json::from_str(&serialized).map_err(|e| e.to_string()))
        .transpose()
}

#[cfg(target_os = "android")]
fn save_secure_config(_app: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    let serialized = serde_json::to_string(config).map_err(|e| e.to_string())?;
    android_secure_config(Some(&serialized)).map(|_| ())
}

fn write_public_config(app: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    let content =
        serde_json::to_string_pretty(&public_config(config)).map_err(|e| e.to_string())?;
    std::fs::write(get_config_path(app), content).map_err(|e| e.to_string())
}

const LOG_SESSION_MARKER: &str = "=== SESSION START ===";
const MAX_LOG_SESSIONS: usize = 5;
const MAX_LOG_ENTRIES: usize = 5000;
#[cfg(target_os = "android")]
const ANDROID_KEEPALIVE_JOURNAL: &str = "keepalive-journal.log";

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
    #[allow(unused_mut)]
    let mut existing = std::fs::read_to_string(&path).unwrap_or_default();
    #[cfg(target_os = "android")]
    let mut imported_journals = Vec::new();
    #[cfg(target_os = "android")]
    if let Some(parent) = path.parent() {
        let journal_path = parent.join(ANDROID_KEEPALIVE_JOURNAL);
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let importing_path = parent.join(format!("keepalive-journal.importing-{suffix}.log"));
        if std::fs::rename(&journal_path, &importing_path).is_ok() {
            imported_journals.push(importing_path);
        }
        // Also recover an import left behind if the previous startup stopped
        // between the atomic rename and the final app.log write.
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let file_name = file_name.to_string_lossy();
                if file_name.starts_with("keepalive-journal.importing")
                    && file_name.ends_with(".log")
                    && !imported_journals.contains(&entry.path())
                {
                    imported_journals.push(entry.path());
                }
            }
        }
        imported_journals.sort();
        imported_journals.retain(|importing_path| {
            if let Ok(journal) = std::fs::read_to_string(importing_path) {
                if !existing.is_empty() && !existing.ends_with('\n') {
                    existing.push('\n');
                }
                existing.push_str(&journal);
                true
            } else {
                false
            }
        });
    }
    let existing_lines: Vec<&str> = existing.lines().collect();
    let session_starts: Vec<usize> = existing_lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| line.contains(LOG_SESSION_MARKER).then_some(index))
        .collect();
    let keep_from = if session_starts.len() >= MAX_LOG_SESSIONS {
        session_starts[session_starts.len() - (MAX_LOG_SESSIONS - 1)]
    } else {
        0
    };
    let mut lines: Vec<String> = existing_lines[keep_from..]
        .iter()
        .map(|line| (*line).to_string())
        .collect();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    lines.push(format!(
        "[{}] [info] [系统] {} {}",
        now, LOG_SESSION_MARKER, now
    ));
    let serialized = format!("{}\n", lines.join("\n"));
    let history_written = std::fs::write(path, serialized).is_ok();
    #[cfg(target_os = "android")]
    if history_written {
        for importing_path in imported_journals {
            let _ = std::fs::remove_file(importing_path);
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = history_written;

    let mut memory = state.logs.lock().unwrap();
    memory.clear();
    memory.extend(lines.iter().filter_map(|line| parse_log_line(line)));
    if memory.len() > MAX_LOG_ENTRIES {
        let remove_count = memory.len() - MAX_LOG_ENTRIES;
        memory.drain(..remove_count);
    }
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
        if let Some(index) = current
            .accounts
            .iter()
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
        if let Some(saved) = existing
            .accounts
            .iter()
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

    match load_secure_config(app) {
        Ok(Some(mut config)) => {
            let migrated = disk_config
                .as_ref()
                .map(|legacy| merge_legacy_credentials(&mut config, legacy))
                .unwrap_or(false);
            let migration_persisted = if migrated {
                if let Err(error) = save_secure_config_verified(app, &config) {
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
                if config
                    .accounts
                    .iter()
                    .any(|account| !account.pass.is_empty())
                {
                    if let Err(error) = save_secure_config_verified(app, &config) {
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

fn save_config(
    app: &tauri::AppHandle,
    state: &AppState,
    mut new_cfg: AppConfig,
) -> Result<(), String> {
    if new_cfg.android_notification_mode != "separate" {
        new_cfg.android_notification_mode = default_android_notification_mode();
    }
    let previous_cfg = {
        let state_cfg = state.config.read().unwrap();
        fill_missing_passwords(&mut new_cfg, &state_cfg);
        state_cfg.clone()
    };
    new_cfg.campus_service_sessions = previous_cfg
        .campus_service_sessions
        .iter()
        .filter(|session| {
            previous_cfg
                .accounts
                .iter()
                .find(|account| account.user == session.account())
                .zip(
                    new_cfg
                        .accounts
                        .iter()
                        .find(|account| account.user == session.account()),
                )
                .is_some_and(|(before, after)| before.pass == after.pass)
        })
        .cloned()
        .collect();
    // Recovery records are backend-owned. The WebView intentionally never
    // receives them, so ordinary setting/account saves must not erase an
    // unfinished payment.
    new_cfg.recharge_transactions = previous_cfg.recharge_transactions.clone();
    let storage_was_unreadable = {
        let status = state.credential_storage_status.lock().unwrap();
        status.as_str() == "error" || status.as_str() == "unknown"
    };
    if storage_was_unreadable
        && new_cfg
            .accounts
            .iter()
            .any(|account| account.pass.is_empty())
    {
        match load_secure_config(app) {
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
    save_secure_config_verified(app, &new_cfg)?;
    *state.credential_storage_status.lock().unwrap() = "available".to_string();
    write_public_config(app, &new_cfg)?;
    {
        let mut state_cfg = state.config.write().unwrap();
        *state_cfg = new_cfg.clone();
    }
    reconcile_account_health_after_config_save(app, state, &previous_cfg, &new_cfg);
    #[cfg(desktop)]
    refresh_tray_menu(app, state);
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
        let is_bg = app_is_in_background(&app, &state);
        state.is_in_background.store(is_bg, Ordering::SeqCst);
        let (interval_fg, interval_bg, compatibility) = {
            let cfg = state.config.read().unwrap();
            (
                cfg.check_interval,
                cfg.check_interval_bg,
                effective_vpn_compatibility(&cfg),
            )
        };
        let _ = app.emit("countdown-tick", serde_json::json!({"status": "checking"}));
        rust_log(
            &app,
            &state,
            "网络",
            &format!(
                "[DEBUG] 开始检测网络连通性 (模式: {})",
                if is_bg { "后台" } else { "前台" }
            ),
            "debug",
        );

        // Connectivity-only checks reuse the last details and avoid location-protected APIs.
        #[cfg(target_os = "macos")]
        if is_bg && !full_details {
            rust_log(
                &app,
                &state,
                "隐私",
                "macOS 后台普通检测已跳过 SSID/BSSID，仅检查网络连通性",
                "debug",
            );
        }
        #[cfg(target_os = "macos")]
        if is_bg && full_details {
            rust_log(
                &app,
                &state,
                "隐私",
                "macOS 后台完整检测已触发，将读取 SSID/BSSID",
                "debug",
            );
        }
        #[cfg(target_os = "android")]
        let net_info = get_network_info(app.clone(), Some(full_details));
        #[cfg(not(target_os = "android"))]
        let net_info = if full_details {
            get_network_info(app.clone(), Some(true))
        } else {
            state.last_network_state.lock().unwrap().clone()
        };
        let previous_network = state.last_network_state.lock().unwrap().clone();
        let transport = network_transport(&net_info).to_string();
        let is_mobile_data = is_mobile_data_network(&net_info);
        let was_mobile_data = is_mobile_data_network(&previous_network);
        let system_validated = network_is_system_validated(&net_info);
        #[cfg(target_os = "android")]
        let metered = net_info
            .get("metered")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let raw_ssid = net_info.get("ssid").and_then(|v| v.as_str()).unwrap_or("");
        let raw_bssid = net_info.get("bssid").and_then(|v| v.as_str()).unwrap_or("");
        let raw_ip = net_info.get("ip").and_then(|v| v.as_str()).unwrap_or("");
        let preserve_wifi_identity = !full_details && transport.eq_ignore_ascii_case("wifi");
        let current_ssid = if preserve_wifi_identity && raw_ssid.is_empty() {
            previous_network
                .get("ssid")
                .and_then(|value| value.as_str())
                .unwrap_or("")
        } else {
            raw_ssid
        }
        .to_string();
        let current_bssid = if preserve_wifi_identity && raw_bssid.is_empty() {
            previous_network
                .get("bssid")
                .and_then(|value| value.as_str())
                .unwrap_or("")
        } else {
            raw_bssid
        }
        .to_string();
        let current_ip = if preserve_wifi_identity && raw_ip.is_empty() {
            previous_network
                .get("ip")
                .and_then(|value| value.as_str())
                .unwrap_or("")
        } else {
            raw_ip
        }
        .to_string();
        let preliminary_profile = {
            let cfg = state.config.read().unwrap();
            matching_network_profile(&cfg, &current_ssid, &current_bssid, &LoginType::Unknown)
        };
        let mut next_interval = if is_bg { interval_bg } else { interval_fg };
        if let Some(profile) = preliminary_profile.as_ref() {
            let interval = if is_bg {
                profile.check_interval_bg
            } else {
                profile.check_interval
            };
            if let Some(interval) = interval.filter(|value| *value >= 5) {
                next_interval = interval;
            }
        }
        if is_mobile_data {
            next_interval = mobile_data_check_interval(next_interval, is_bg);
            rust_log(
                &app,
                &state,
                "网络",
                &format!(
                    "{}移动数据网络，自动检测间隔为 {} 秒，校园网认证网关探测已停用",
                    if was_mobile_data {
                        "继续使用"
                    } else {
                        "检测到"
                    },
                    next_interval
                ),
                if was_mobile_data { "debug" } else { "info" },
            );
        }
        state.countdown.store(next_interval, Ordering::SeqCst);
        let local_now = chrono::Local::now();
        let timestamp = local_now.format("%Y-%m-%d %H:%M:%S").to_string();

        if full_details {
            #[cfg(target_os = "android")]
            rust_log(
                &app,
                &state,
                "网络",
                &format!(
                    "[DEBUG] 完整检测网络详情: SSID={}, BSSID={}, IP={}, 传输类型={}, 系统验证={}",
                    current_ssid, current_bssid, current_ip, transport, system_validated
                ),
                "debug",
            );
            #[cfg(not(target_os = "android"))]
            rust_log(
                &app,
                &state,
                "网络",
                &format!(
                    "[DEBUG] 完整检测网络详情: SSID={}, BSSID={}, IP={}",
                    current_ssid, current_bssid, current_ip
                ),
                "debug",
            );
        } else {
            #[cfg(target_os = "android")]
            rust_log(
                &app,
                &state,
                "网络",
                &format!("[DEBUG] 后台间隔检测仅检查连通性，传输类型={}", transport),
                "debug",
            );
            #[cfg(not(target_os = "android"))]
            rust_log(
                &app,
                &state,
                "网络",
                "[DEBUG] 后台间隔检测仅检查连通性，复用上次网络详情",
                "debug",
            );
        }

        let make_payload = |state_str: &str, login_type: Option<&LoginType>| {
            #[allow(unused_mut)]
            let mut payload = serde_json::json!({
                "state": state_str,
                "loginType": login_type.map(LoginType::as_str),
                "ssid": current_ssid.clone(),
                "bssid": current_bssid.clone(),
                "ip": current_ip.clone(),
                "timestamp": timestamp.clone()
            });
            #[cfg(target_os = "android")]
            if let Some(object) = payload.as_object_mut() {
                object.insert(
                    "transport".to_string(),
                    serde_json::json!(transport.clone()),
                );
                object.insert("validated".to_string(), serde_json::json!(system_validated));
                object.insert("metered".to_string(), serde_json::json!(metered));
            }
            payload
        };

        let is_online = if system_validated {
            rust_log(
                &app,
                &state,
                "网络",
                "[DEBUG] 系统已验证默认网络具备互联网能力，跳过额外连通性探测",
                "debug",
            );
            true
        } else {
            check_internet_rust().await
        };
        rust_log(
            &app,
            &state,
            "网络",
            &format!(
                "[DEBUG] 互联网可用性检测结果: {}",
                if is_online {
                    "连通 (Online)"
                } else {
                    "断开/受限"
                }
            ),
            "debug",
        );

        if is_online {
            rust_log(
                &app,
                &state,
                "网络",
                "网络检测完毕: 互联网已连通 (Online)",
                "info",
            );
            state.is_checking.store(false, Ordering::SeqCst);
            state.non_campus_count.store(0, Ordering::SeqCst);
            let payload = make_payload("Online", None);
            {
                let mut last_state = state.last_network_state.lock().unwrap();
                *last_state = payload.clone();
            }
            let _ = app.emit("network-state-change", payload);
            #[cfg(desktop)]
            refresh_tray_menu(&app, &state);
            return;
        }

        if is_mobile_data {
            rust_log(
                &app,
                &state,
                "网络",
                "移动数据未通过互联网连通性验证；已跳过全部校园网认证网关探测与自动登录",
                "info",
            );
            state.is_checking.store(false, Ordering::SeqCst);
            state.non_campus_count.store(0, Ordering::SeqCst);
            let payload = make_payload("Offline", None);
            {
                let mut last_state = state.last_network_state.lock().unwrap();
                *last_state = payload.clone();
            }
            let _ = app.emit("network-state-change", payload);
            #[cfg(desktop)]
            refresh_tray_menu(&app, &state);
            return;
        }

        rust_log(
            &app,
            &state,
            "网络",
            &format!(
                "[DEBUG] 使用 VPN 共存兼容等级 {} 探测校园网网关",
                compatibility.as_str()
            ),
            "debug",
        );
        let detected_login_type = detect_login_type_rust(compatibility).await;
        let profile = {
            let cfg = state.config.read().unwrap();
            matching_network_profile(&cfg, &current_ssid, &current_bssid, &detected_login_type)
        };
        let login_type = profile
            .as_ref()
            .and_then(|item| login_type_from_profile(&item.login_type))
            .unwrap_or_else(|| detected_login_type.clone());
        if let Some(profile) = profile.as_ref() {
            rust_log(
                &app,
                &state,
                "网络档案",
                &format!("已匹配网络档案“{}”，使用其账号顺序与登录策略", profile.name),
                "info",
            );
            let profile_interval = if is_bg {
                profile.check_interval_bg
            } else {
                profile.check_interval
            };
            if let Some(interval) = profile_interval.filter(|value| *value >= 5) {
                state.countdown.store(interval, Ordering::SeqCst);
            }
        }
        rust_log(
            &app,
            &state,
            "网络",
            &format!(
                "[DEBUG] 检测到校园网环境判定: {}",
                if login_type != LoginType::Unknown {
                    "需要登录认证"
                } else {
                    "非校园网/完全离线"
                }
            ),
            "debug",
        );

        match login_type {
            LoginType::Unknown => {
                state.non_campus_count.store(0, Ordering::SeqCst);
                rust_log(
                    &app,
                    &state,
                    "网络",
                    "网络检测完毕: 离线或非校园网 (Offline)",
                    "info",
                );
                let payload = make_payload("Offline", None);
                {
                    let mut last_state = state.last_network_state.lock().unwrap();
                    *last_state = payload.clone();
                }
                let _ = app.emit("network-state-change", payload);
            }
            _ => {
                rust_log(
                    &app,
                    &state,
                    "网络",
                    &format!("检测到校园网登录页面 (登录类型: {:?})", login_type),
                    "info",
                );
                let mut login_succeeded = false;
                let auto_login_paused = state.auto_login_paused_until.load(Ordering::SeqCst)
                    > chrono::Utc::now().timestamp();
                let auto_login_enabled = {
                    let cfg = state.config.read().unwrap();
                    profile_auto_login_enabled(profile.as_ref(), &login_type, cfg.auto_login)
                };
                if auto_login_enabled && !auto_login_paused {
                    let (whitelist, blacklist) = {
                        let cfg = state.config.read().unwrap();
                        (cfg.whitelist.clone(), cfg.blacklist.clone())
                    };
                    let proceed = match automatic_login_network_allowed(
                        &login_type,
                        &current_ssid,
                        &current_bssid,
                        &current_ip,
                        &transport,
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
                        for account in accounts_for_profile(accounts, profile.as_ref()) {
                            match account_attempt_allowed(&state, &account.user) {
                                Ok(()) => active_accounts.push(account),
                                Err(remaining) => rust_log(
                                    &app,
                                    &state,
                                    "账号健康",
                                    &format!(
                                        "账号 {} 仍在冷却中（剩余 {} 秒），跳过本次尝试",
                                        account.user, remaining
                                    ),
                                    "info",
                                ),
                            }
                        }
                        if active_accounts.is_empty() {
                            rust_log(
                                &app,
                                &state,
                                "网络",
                                "未配置带已保存密码的有效账号，跳过自动登录",
                                "error",
                            );
                        } else {
                            let mut success = false;
                            for acc in active_accounts {
                                rust_log(
                                    &app,
                                    &state,
                                    "网络",
                                    &format!("尝试使用账号 {} 自动登录...", acc.user),
                                    "info",
                                );
                                match login_to_campus_network_rust(
                                    login_type.clone(),
                                    &acc.user,
                                    &acc.pass,
                                    compatibility,
                                )
                                .await
                                {
                                    Ok((true, msg)) => {
                                        record_account_success(&app, &state, &acc.user);
                                        rust_log(
                                            &app,
                                            &state,
                                            "网络",
                                            &format!("登录成功: {}", msg),
                                            "success",
                                        );
                                        if automatic_login_result_notifications_enabled(&state) {
                                            let _ = show_native_notification(
                                                &app,
                                                "自动登录成功",
                                                &format!("账号: {}", acc.user),
                                            );
                                        }
                                        success = true;
                                        login_succeeded = true;
                                        break;
                                    }
                                    Ok((false, msg)) => {
                                        record_account_failure(&app, &state, &acc.user, &msg);
                                        rust_log(
                                            &app,
                                            &state,
                                            "网络",
                                            &format!("登录失败: {}", msg),
                                            "error",
                                        );
                                    }
                                    Err(err) => {
                                        record_account_failure(
                                            &app,
                                            &state,
                                            &acc.user,
                                            &format!("请求出错: {err}"),
                                        );
                                        rust_log(
                                            &app,
                                            &state,
                                            "网络",
                                            &format!("请求出错: {}", err),
                                            "error",
                                        );
                                    }
                                }
                            }
                            if !success {
                                rust_log(
                                    &app,
                                    &state,
                                    "网络",
                                    "所有账号登录尝试完毕，均未成功",
                                    "error",
                                );
                            }
                        }
                    }
                } else if auto_login_paused {
                    rust_log(&app, &state, "网络", "自动登录已临时暂停，忽略重连", "info");
                } else {
                    rust_log(&app, &state, "网络", "自动登录未开启，忽略重连", "info");
                }
                if login_succeeded {
                    state.non_campus_count.store(0, Ordering::SeqCst);
                } else if is_bg {
                    let count = state.non_campus_count.fetch_add(1, Ordering::SeqCst) + 1;
                    rust_log(
                        &app,
                        &state,
                        "网络",
                        &format!("[DEBUG] 后台检测为非校园网环境，当前连续次数: {}/5", count),
                        "debug",
                    );
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
        #[cfg(desktop)]
        refresh_tray_menu(&app, &state);
    });
}

#[tauri::command]
fn sync_config(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    config: AppConfig,
) -> Result<(), String> {
    if let Err(error) = save_config(&app, &state, config) {
        rust_log(
            &app,
            &state,
            "配置",
            &format!("配置持久化失败: {error}"),
            "error",
        );
        return Err(error);
    }
    rust_log(&app, &state, "配置", "配置已写入安全存储", "debug");
    let is_bg = state.is_in_background.load(Ordering::SeqCst);
    let current_val = state.countdown.load(Ordering::SeqCst);
    let new_cfg = state.config.read().unwrap();
    let configured_interval = if is_bg {
        new_cfg.check_interval_bg
    } else {
        new_cfg.check_interval
    };
    let mobile_data = is_mobile_data_network(&state.last_network_state.lock().unwrap());
    let new_interval = if mobile_data {
        mobile_data_check_interval(configured_interval, is_bg)
    } else {
        configured_interval
    };
    if current_val > new_interval {
        state.countdown.store(new_interval, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
fn get_app_config(state: tauri::State<Arc<AppState>>) -> serde_json::Value {
    let config = state.config.read().unwrap().clone();
    let mut value = serde_json::to_value(&config).unwrap_or_default();
    if let Some(accounts) = value
        .get_mut("accounts")
        .and_then(serde_json::Value::as_array_mut)
    {
        for (view, saved) in accounts.iter_mut().zip(config.accounts.iter()) {
            if let Some(object) = view.as_object_mut() {
                object.insert("pass".to_string(), serde_json::Value::String(String::new()));
                object.insert(
                    "hasPassword".to_string(),
                    serde_json::Value::Bool(!saved.pass.is_empty()),
                );
            }
        }
    }
    if let Some(object) = value.as_object_mut() {
        object.insert("campusServiceSessions".to_string(), serde_json::json!([]));
        object.insert("rechargeTransactions".to_string(), serde_json::json!([]));
    }
    value
}

fn credential_snapshot_fingerprint(config: &AppConfig, users: &[String]) -> Option<String> {
    use base64::Engine;
    use sha2::Digest;

    #[derive(serde::Serialize)]
    struct CredentialSnapshot<'a> {
        user: &'a str,
        pass: &'a str,
    }

    if users.is_empty() {
        return None;
    }
    let mut snapshot = Vec::with_capacity(users.len());
    for user in users {
        let account = config
            .accounts
            .iter()
            .find(|account| account.user == *user)?;
        if account.pass.is_empty() {
            return None;
        }
        snapshot.push(CredentialSnapshot {
            user: &account.user,
            pass: &account.pass,
        });
    }
    let serialized = serde_json::to_vec(&snapshot).ok()?;
    let digest = sha2::Sha256::digest(serialized);
    Some(base64::engine::general_purpose::STANDARD.encode(digest))
}

#[tauri::command]
fn verify_legacy_credential_fingerprint(
    state: tauri::State<Arc<AppState>>,
    users: Vec<String>,
    fingerprint: String,
) -> bool {
    if fingerprint.trim().is_empty() {
        return false;
    }
    let config = state.config.read().unwrap();
    credential_snapshot_fingerprint(&config, &users).is_some_and(|actual| actual == fingerprint)
}

#[tauri::command]
fn get_account_password(
    state: tauri::State<Arc<AppState>>,
    user: String,
) -> Result<String, String> {
    let user = user.trim();
    let config = state.config.read().unwrap();
    config
        .accounts
        .iter()
        .find(|account| account.user == user)
        .map(|account| account.pass.clone())
        .filter(|password| !password.is_empty())
        .ok_or_else(|| "该账号没有可读取的已保存密码".to_string())
}

#[tauri::command]
fn get_credential_storage_status(state: tauri::State<Arc<AppState>>) -> String {
    state.credential_storage_status.lock().unwrap().clone()
}

fn credential_backend_name() -> &'static str {
    if cfg!(target_os = "android") {
        "Android Keystore (AES-GCM)"
    } else if cfg!(target_os = "macos") {
        "macOS 本地加密文件 (AES-GCM)"
    } else if cfg!(target_os = "windows") {
        "Windows Credential Manager"
    } else if cfg!(target_os = "linux") {
        "Linux Secret Service"
    } else {
        "安全凭据存储"
    }
}

#[tauri::command]
fn get_credential_storage_health(state: tauri::State<Arc<AppState>>) -> CredentialStorageHealth {
    let status = state.credential_storage_status.lock().unwrap().clone();
    let config = state.config.read().unwrap();
    let missing_password_accounts: Vec<String> = config
        .accounts
        .iter()
        .filter(|account| account.pass.is_empty())
        .map(|account| account.user.clone())
        .collect();
    let saved_accounts = config
        .accounts
        .len()
        .saturating_sub(missing_password_accounts.len());
    let message = match status.as_str() {
        "available" if missing_password_accounts.is_empty() => {
            "安全凭据存储工作正常，所有账号均已保存密码"
        }
        "available" => "安全凭据存储可用，但部分账号仍需补录密码",
        "missing" => "安全凭据存储可用，当前尚未保存凭据",
        "error" => "安全凭据存储暂时不可读；应用已阻止空密码覆盖",
        _ => "安全凭据存储状态尚未完成初始化",
    }
    .to_string();
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

    let compatibility = app
        .try_state::<Arc<AppState>>()
        .map(|state| {
            let config = state.config.read().unwrap();
            effective_vpn_compatibility(&config)
        })
        .unwrap_or(VpnCompatibility::High);
    let mut steps = Vec::new();
    let identity_started = std::time::Instant::now();
    let network = get_network_info(app, Some(true));
    let transport = network_transport(&network).to_string();
    let mobile_data = is_mobile_data_network(&network);
    let system_validated = network_is_system_validated(&network);
    let ssid = network
        .get("ssid")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let mut ip = network
        .get("ip")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if ip.is_empty() {
        ip = get_local_ip();
    }
    let identity_status = if ip.is_empty() {
        "error"
    } else if mobile_data || !ssid.is_empty() {
        "success"
    } else {
        "warning"
    };
    let identity_message = if ip.is_empty() {
        "未检测到可用网络接口或 IPv4 地址".to_string()
    } else if mobile_data {
        format!("当前通过移动数据上网，本地 IP {ip}")
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
    let campus_status = if mobile_data {
        "skipped"
    } else if campus_ip && (campus_ssid || ssid.is_empty() || ssid.contains("unknown")) {
        "success"
    } else if campus_ip || campus_ssid {
        "warning"
    } else {
        "error"
    };
    let campus_message = if mobile_data {
        "移动数据模式不属于校园局域网，已停用校园网环境判定".to_string()
    } else {
        match (campus_ip, campus_ssid) {
            (true, true) => "SSID 与本地网段均符合校园网特征".to_string(),
            (true, false) => {
                "本地网段符合校园网，但 SSID 未识别；自动登录需要白名单或有线协议".to_string()
            }
            (false, true) => "SSID 符合校园网，但本地 IP 不在已知网段".to_string(),
            (false, false) => "当前网络不符合已知校园网特征".to_string(),
        }
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
        ("www.baidu.com", 443)
            .to_socket_addrs()
            .map(|mut addresses| addresses.next().is_some())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false);
    steps.push(make_diagnostic_step(
        "dns",
        "DNS 解析",
        dns_started,
        if dns_ok { "success" } else { "warning" },
        if dns_ok {
            "DNS 解析正常".to_string()
        } else {
            "DNS 解析失败或被认证页面限制".to_string()
        },
    ));

    let internet_started = std::time::Instant::now();
    let online = system_validated || check_internet_rust().await;
    steps.push(make_diagnostic_step(
        "internet",
        "互联网连通性",
        internet_started,
        if online { "success" } else { "warning" },
        if system_validated {
            "Android 已验证默认网络具备互联网能力".to_string()
        } else if online {
            "多个独立探测目标中至少一个验证成功，互联网访问正常".to_string()
        } else if mobile_data {
            "移动数据未通过互联网验证；不会继续探测校园网认证网关".to_string()
        } else {
            "多个独立目标均未验证成功，继续检测认证网关".to_string()
        },
    ));

    let portal_started = std::time::Instant::now();
    let login_type = if online || mobile_data {
        LoginType::Unknown
    } else {
        detect_login_type_rust(compatibility).await
    };
    let (portal_status, portal_message) = if mobile_data {
        (
            "skipped",
            "当前使用移动数据，已跳过校园网认证网关探测".to_string(),
        )
    } else if online {
        ("success", "互联网已连通，跳过认证网关探测".to_string())
    } else if login_type != LoginType::Unknown {
        (
            "warning",
            format!(
                "已检测到校园网认证协议 {}，当前需要登录",
                login_type.as_str()
            ),
        )
    } else {
        (
            "error",
            "未找到可访问的校园网认证网关，可能是完全离线或处于非校园网络".to_string(),
        )
    };
    steps.push(make_diagnostic_step(
        "portal",
        "认证网关",
        portal_started,
        portal_status,
        portal_message,
    ));

    let (overall, summary) = if online && mobile_data {
        ("healthy", "移动数据网络工作正常；校园网探测已停用")
    } else if online {
        ("healthy", "网络工作正常，互联网已连通")
    } else if login_type != LoginType::Unknown {
        ("auth_required", "已连接校园网，但需要完成账号认证")
    } else if ip.is_empty() || transport == "none" {
        (
            "no_network",
            "未取得网络地址，请检查 Wi-Fi、有线连接或系统权限",
        )
    } else if mobile_data {
        (
            "offline",
            "移动数据存在，但系统与独立目标均未验证互联网连通性",
        )
    } else {
        (
            "offline",
            "已取得本地网络，但无法访问互联网或校园网认证网关",
        )
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

fn mask_identifier(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 4 {
        return "****".to_string();
    }
    format!(
        "{}***{}",
        chars.iter().take(2).collect::<String>(),
        chars
            .iter()
            .rev()
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>()
    )
}

fn redact_diagnostic_text(mut text: String, accounts: &[Account], ip: &str, bssid: &str) -> String {
    for account in accounts {
        if !account.user.is_empty() {
            text = text.replace(&account.user, &mask_identifier(&account.user));
        }
        if !account.pass.is_empty() {
            text = text.replace(&account.pass, "[REDACTED]");
        }
    }
    if !ip.is_empty() {
        text = text.replace(ip, "[LOCAL-IP]");
    }
    if !bssid.is_empty() {
        text = text.replace(bssid, "[BSSID]");
    }
    text
}

#[tauri::command]
async fn create_diagnostic_bundle(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<String, String> {
    let mut report = run_network_diagnostics(app.clone()).await;
    let config = state.config.read().unwrap().clone();
    let network_state = state.last_network_state.lock().unwrap().clone();
    let ip = network_state
        .get("ip")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let bssid = network_state
        .get("bssid")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let logs: Vec<serde_json::Value> = state.logs.lock().unwrap().iter().rev().take(500).rev()
        .map(|entry| serde_json::json!({
            "time": entry.time,
            "module": entry.module,
            "type": entry.log_type,
            "message": redact_diagnostic_text(entry.message.clone(), &config.accounts, ip, bssid),
        }))
        .collect();
    let public_accounts: Vec<Account> = config
        .accounts
        .iter()
        .cloned()
        .map(|mut account| {
            account.pass.clear();
            account
        })
        .collect();
    report.summary = redact_diagnostic_text(report.summary, &public_accounts, ip, bssid);
    report.ip = if report.ip.is_empty() {
        String::new()
    } else {
        "[LOCAL-IP]".to_string()
    };
    for step in &mut report.steps {
        step.message = redact_diagnostic_text(
            std::mem::take(&mut step.message),
            &public_accounts,
            ip,
            bssid,
        );
    }
    let redacted_report = serde_json::to_value(&report).map_err(|error| error.to_string())?;
    let bundle = serde_json::json!({
        "schemaVersion": 1,
        "appVersion": app.package_info().version.to_string(),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "createdAt": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        "configuration": {
            "accountCount": config.accounts.len(),
            "enabledAccountCount": config.accounts.iter().filter(|account| !account.is_disabled.unwrap_or(false)).count(),
            "networkProfileCount": config.network_profiles.len(),
            "autoLogin": config.auto_login,
            "checkInterval": config.check_interval,
            "checkIntervalBackground": config.check_interval_bg,
            "usageAlerts": config.usage_alerts,
        },
        "diagnostic": redacted_report,
        "logs": logs,
        "privacy": "账号、密码、本地 IP 与 BSSID 已脱敏；诊断包不包含凭据。",
    });
    serde_json::to_string_pretty(&bundle).map_err(|error| error.to_string())
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
fn export_logs(app: tauri::AppHandle) -> Result<String, String> {
    let source = get_log_path(&app);
    let metadata = std::fs::metadata(&source).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "当前没有可导出的日志".to_string()
        } else {
            format!("读取日志失败：{error}")
        }
    })?;
    if metadata.len() == 0 {
        return Err("当前没有可导出的日志".to_string());
    }
    let directory = app
        .path()
        .download_dir()
        .or_else(|_| app.path().document_dir())
        .map_err(|error| format!("无法定位导出目录：{error}"))?;
    std::fs::create_dir_all(&directory).map_err(|error| format!("无法创建导出目录：{error}"))?;
    let filename = format!(
        "BJUT-AL-logs-{}.log",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let destination = directory.join(filename);
    std::fs::copy(&source, &destination).map_err(|error| format!("导出日志失败：{error}"))?;
    Ok(destination.to_string_lossy().into_owned())
}

#[tauri::command]
fn export_billing_csv(app: tauri::AppHandle, kind: String, csv: String) -> Result<String, String> {
    let stem = match kind.as_str() {
        "usage" => "usage",
        "monthly" => "monthly",
        "payments" => "payments",
        "operations" => "operations",
        "stopLogs" => "stop-logs",
        "reopenLogs" => "reopen-logs",
        "packageLogs" => "package-logs",
        _ => return Err("不支持的账单导出类型".to_string()),
    };
    if csv.trim().is_empty() {
        return Err("当前没有可导出的账单记录".to_string());
    }
    if csv.len() > 32 * 1024 * 1024 {
        return Err("账单导出内容超过 32 MiB，请缩小查询日期范围".to_string());
    }

    #[cfg(target_os = "android")]
    let directory = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("无法定位导出缓存目录：{error}"))?
        .join("exports");
    #[cfg(not(target_os = "android"))]
    let directory = app
        .path()
        .download_dir()
        .or_else(|_| app.path().document_dir())
        .map_err(|error| format!("无法定位导出目录：{error}"))?;

    std::fs::create_dir_all(&directory).map_err(|error| format!("无法创建导出目录：{error}"))?;
    let filename = format!(
        "BJUT-AL-billing-{stem}-{}.csv",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let destination = directory.join(filename);
    std::fs::write(&destination, csv.as_bytes())
        .map_err(|error| format!("写入账单 CSV 失败：{error}"))?;
    Ok(destination.to_string_lossy().into_owned())
}

#[tauri::command]
fn clear_all_logs(app: tauri::AppHandle, state: tauri::State<Arc<AppState>>) {
    state.logs.lock().unwrap().clear();
    let p = get_log_path(&app);
    let _ = std::fs::remove_file(p);
    #[cfg(target_os = "android")]
    if let Some(parent) = get_log_path(&app).parent() {
        let _ = std::fs::remove_file(parent.join(ANDROID_KEEPALIVE_JOURNAL));
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let file_name = file_name.to_string_lossy();
                if file_name.starts_with("keepalive-journal.importing")
                    && file_name.ends_with(".log")
                {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
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
    rust_log(
        &app,
        &state,
        "网络",
        "收到手动连通性检测请求，开始检测...",
        "info",
    );
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
    let network = get_network_info(app.clone(), Some(true));
    if is_mobile_data_network(&network) {
        rust_log(
            &app,
            &state,
            "登录",
            "当前使用移动数据，已阻止校园网网关探测和手动登录",
            "info",
        );
        return Ok(ManualLoginResult {
            success: false,
            message: "当前使用移动数据，未连接 Wi-Fi；已停止校园网网关探测".to_string(),
        });
    }
    let compatibility = {
        let config = state.config.read().unwrap();
        effective_vpn_compatibility(&config)
    };
    let detected_type = detect_login_type_rust(compatibility).await;
    let login_type = match login_type_override.as_deref() {
        Some("bjut_sushe") | Some("bjut-sushe") => LoginType::Type1,
        Some("bjut-wifi") | Some("bjut_wifi") => LoginType::Type2,
        Some("wired") => LoginType::Type3,
        _ => detected_type,
    };
    if login_type == LoginType::Unknown {
        return Ok(ManualLoginResult {
            success: false,
            message: "未检测到校园网登录页面".to_string(),
        });
    }

    let configured_accounts = state.config.read().unwrap().accounts.clone();
    let accounts: Vec<Account> = match account_index {
        Some(index) => configured_accounts
            .get(index)
            .cloned()
            .into_iter()
            .collect(),
        None => configured_accounts,
    }
    .into_iter()
    .filter(|account| !account.is_disabled.unwrap_or(false))
    .collect();
    if accounts.is_empty() {
        return Ok(ManualLoginResult {
            success: false,
            message: "未配置可用账号".to_string(),
        });
    }

    for account in accounts {
        if account.pass.is_empty() {
            rust_log(
                &app,
                &state,
                "登录",
                &format!("账号 {} 缺少已保存的密码", account.user),
                "error",
            );
            continue;
        }
        if let Err(remaining) = account_attempt_allowed(&state, &account.user) {
            rust_log(
                &app,
                &state,
                "账号健康",
                &format!(
                    "账号 {} 正在冷却，剩余 {} 秒；可在网络诊断页解除",
                    account.user, remaining
                ),
                "info",
            );
            continue;
        }
        rust_log(
            &app,
            &state,
            "登录",
            &format!("尝试使用账号 {} 登录...", account.user),
            "info",
        );
        match login_to_campus_network_rust(
            login_type.clone(),
            &account.user,
            &account.pass,
            compatibility,
        )
        .await
        {
            Ok((true, message)) => {
                record_account_success(&app, &state, &account.user);
                rust_log(
                    &app,
                    &state,
                    "登录",
                    &format!("登录成功: {message}"),
                    "success",
                );
                let net_info = get_network_info(app.clone(), Some(true));
                let payload = serde_json::json!({
                    "state": "Online",
                    "ssid": net_info.get("ssid").and_then(|v| v.as_str()).unwrap_or(""),
                    "bssid": net_info.get("bssid").and_then(|v| v.as_str()).unwrap_or(""),
                    "ip": net_info.get("ip").and_then(|v| v.as_str()).unwrap_or(""),
                    "timestamp": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
                });
                *state.last_network_state.lock().unwrap() = payload.clone();
                let _ = app.emit("network-state-change", payload);
                #[cfg(desktop)]
                refresh_tray_menu(&app, &state);
                return Ok(ManualLoginResult {
                    success: true,
                    message,
                });
            }
            Ok((false, message)) => {
                record_account_failure(&app, &state, &account.user, &message);
                rust_log(
                    &app,
                    &state,
                    "登录",
                    &format!("登录失败: {message}"),
                    "error",
                );
            }
            Err(error) => {
                record_account_failure(&app, &state, &account.user, &format!("请求出错: {error}"));
                rust_log(&app, &state, "登录", &format!("请求出错: {error}"), "error");
            }
        }
    }
    Ok(ManualLoginResult {
        success: false,
        message: "所有可用账号均未能登录".to_string(),
    })
}

fn first_decimal(value: &str) -> Option<f64> {
    let number = value
        .chars()
        .filter(|character| character.is_ascii_digit() || *character == '.')
        .collect::<String>();
    number.parse::<f64>().ok()
}

fn evaluate_usage_alerts(app: &tauri::AppHandle, state: &AppState, info: &UserInfo) {
    let (enabled, balance_threshold, flow_threshold) = {
        let config = state.config.read().unwrap();
        (
            config.usage_alerts,
            config.balance_alert_threshold,
            config.flow_alert_threshold,
        )
    };
    if !enabled {
        return;
    }
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut alerts = Vec::new();
    if let Some(balance) = first_decimal(&info.balance) {
        if balance <= balance_threshold {
            alerts.push((
                "balance",
                format!(
                    "校园网余额仅剩 {}，已低于 {:.2} 元提醒线",
                    info.balance, balance_threshold
                ),
            ));
        }
    }
    if info.flow != "无限" {
        if let Some(flow) = first_decimal(&info.flow) {
            if flow <= flow_threshold {
                alerts.push((
                    "flow",
                    format!(
                        "套餐流量仅剩 {}，已低于 {:.2} GB 提醒线",
                        info.flow, flow_threshold
                    ),
                ));
            }
        }
    }
    for (kind, message) in alerts {
        let should_notify = {
            let mut history = state.usage_alert_history.lock().unwrap();
            if history.get(kind) == Some(&today) {
                false
            } else {
                history.insert(kind.to_string(), today.clone());
                true
            }
        };
        if should_notify {
            rust_log(app, state, "用量提醒", &message, "error");
            let _ = show_native_notification(app, "校园网用量提醒", &message);
            let _ = app.emit(
                "usage-alert",
                serde_json::json!({ "kind": kind, "message": message }),
            );
        }
    }
}

fn preferred_billing_account(config: &AppConfig) -> Option<Account> {
    config
        .accounts
        .iter()
        .filter(|account| !account.is_disabled.unwrap_or(false) && !account.user.trim().is_empty())
        .find(|account| account.is_default)
        .or_else(|| {
            config.accounts.iter().find(|account| {
                !account.is_disabled.unwrap_or(false) && !account.user.trim().is_empty()
            })
        })
        .cloned()
}

fn selected_billing_account(config: &AppConfig, account_user: Option<&str>) -> Option<Account> {
    let requested = account_user
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match requested {
        Some(user) => config
            .accounts
            .iter()
            .find(|account| {
                account.user == user
                    && !account.is_disabled.unwrap_or(false)
                    && !account.user.trim().is_empty()
            })
            .cloned(),
        None => preferred_billing_account(config),
    }
}

fn emit_billing_center_progress(
    app: &tauri::AppHandle,
    state: &AppState,
    message: &str,
    percent: u8,
) {
    rust_log(app, state, "计费", message, "info");
    let _ = app.emit(
        "billing-center-progress",
        serde_json::json!({ "message": message, "percent": percent.min(100) }),
    );
}

#[tauri::command]
async fn discover_current_campus_account(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Option<serde_json::Value>, String> {
    *state.pending_discovered_account.lock().await = None;
    if ensure_billing_foreground(&state).is_err() {
        return Ok(None);
    }
    let compatibility = {
        let config = state.config.read().unwrap();
        effective_vpn_compatibility(&config)
    };
    let request = tokio::time::timeout(
        std::time::Duration::from_secs(25),
        billing::discover_current_campus_account(compatibility),
    );
    let background = billing_runtime::wait_for_background(&state.is_in_background);
    futures_util::pin_mut!(request, background);
    let result = match futures_util::future::select(request, background).await {
        futures_util::future::Either::Left((result, _)) => Some(result),
        futures_util::future::Either::Right((_, _)) => None,
    };
    let Some(result) = result else {
        rust_log(
            &app,
            &state,
            "账号",
            "App 已进入后台，已停止校园网账号发现",
            "debug",
        );
        return Ok(None);
    };
    match result {
        Ok(Ok(Some(account))) => {
            rust_log(
                &app,
                &state,
                "账号",
                "已识别当前校园网会话中的账号，等待用户确认是否保存",
                "info",
            );
            let user = account.user.clone();
            let token_seed = format!(
                "{}:{}:{:?}",
                user,
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
                std::thread::current().id()
            );
            let token = format!("{:x}", md5::compute(token_seed));
            *state.pending_discovered_account.lock().await = Some(PendingDiscoveredAccount {
                account: Account {
                    user: account.user,
                    pass: account.pass,
                    is_default: false,
                    is_disabled: Some(false),
                },
                token: token.clone(),
                expires_at: std::time::Instant::now() + std::time::Duration::from_secs(120),
            });
            Ok(Some(serde_json::json!({ "user": user, "token": token })))
        }
        Ok(Ok(None)) => Ok(None),
        Ok(Err(billing::BillingError::Network(detail))) => {
            rust_log(
                &app,
                &state,
                "账号",
                &format!("当前网络未提供校园网账号发现入口：{detail}"),
                "debug",
            );
            Ok(None)
        }
        Ok(Err(error)) => {
            let message = error.user_message();
            rust_log(&app, &state, "账号", &message, "debug");
            Err(message)
        }
        Err(_) => {
            rust_log(&app, &state, "账号", "校园网账号发现超时", "debug");
            Ok(None)
        }
    }
}

#[tauri::command]
async fn accept_discovered_campus_account(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    user: String,
    token: String,
) -> Result<(), String> {
    let pending = state
        .pending_discovered_account
        .lock()
        .await
        .take()
        .filter(|pending| {
            pending.account.user == user
                && pending.token == token
                && std::time::Instant::now() <= pending.expires_at
        })
        .ok_or_else(|| "待保存的校园网账号凭据已失效，请重新检测".to_string())?;
    let discovered = pending.account;
    let mut config = state.config.read().unwrap().clone();
    if config
        .accounts
        .iter()
        .any(|account| account.user == discovered.user)
    {
        return Ok(());
    }
    let mut discovered = discovered;
    discovered.is_default = config.accounts.is_empty();
    config.accounts.push(discovered);
    save_config(&app, &state, config)?;
    rust_log(
        &app,
        &state,
        "账号",
        "已将发现的校园网账号直接写入安全存储",
        "success",
    );
    Ok(())
}

#[tauri::command]
async fn reject_discovered_campus_account(
    state: tauri::State<'_, Arc<AppState>>,
    token: String,
) -> Result<(), String> {
    let mut pending = state.pending_discovered_account.lock().await;
    if pending
        .as_ref()
        .is_some_and(|candidate| candidate.token == token)
    {
        *pending = None;
    }
    Ok(())
}

#[tauri::command]
async fn get_user_info(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    local_ip: Option<String>,
    force: Option<bool>,
) -> Result<Option<UserInfo>, String> {
    let compatibility = {
        let config = state.config.read().unwrap();
        effective_vpn_compatibility(&config)
    };
    let _ = force;
    let info = fetch_portal_user_info(local_ip.as_deref(), compatibility).await;
    if let Some(info) = info.as_ref() {
        evaluate_usage_alerts(&app, &state, info);
    }
    Ok(info)
}

#[tauri::command]
async fn get_billing_center(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    account_user: Option<String>,
) -> Result<billing::BillingCenterData, String> {
    let (account, compatibility) = billing_action_target(&state, account_user.as_deref())?;
    emit_billing_center_progress(&app, &state, "准备读取计费中心完整数据", 2);
    let _fetch_guard = match state.billing_fetch_lock.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            emit_billing_center_progress(&app, &state, "正在等待已有的计费刷新完成", 3);
            match tokio::time::timeout(
                std::time::Duration::from_secs(50),
                state.billing_fetch_lock.lock(),
            )
            .await
            {
                Ok(guard) => guard,
                Err(_) => {
                    let error = "等待已有计费刷新超过 50 秒，请稍后重试".to_string();
                    rust_log(&app, &state, "计费", &error, "error");
                    return Err(error);
                }
            }
        }
    };
    let progress_app = app.clone();
    let progress_state = state.inner().clone();
    let emit_progress = move |message: &str, percent: u8| {
        emit_billing_center_progress(&progress_app, &progress_state, message, percent);
    };
    ensure_billing_foreground(&state)?;
    let result = run_billing_read_while_foreground(&state, async {
        match tokio::time::timeout(
            std::time::Duration::from_secs(75),
            billing::fetch_center(&account.user, &account.pass, compatibility, emit_progress),
        )
        .await
        {
            Ok(result) => result.map_err(|error| error.user_message()),
            Err(_) => {
                Err("计费中心完整数据读取超过 75 秒，已停止本次请求；请检查 VPN 后重试".to_string())
            }
        }
    })
    .await;
    match &result {
        Ok(data) => rust_log(
            &app,
            &state,
            "计费",
            &format!(
                "计费中心完整数据已更新（{} 条警告）",
                data.warnings.len() + data.overview.warnings.len()
            ),
            "debug",
        ),
        Err(error) => rust_log(&app, &state, "计费", error, "error"),
    }
    result
}

#[tauri::command]
async fn query_billing_records(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    query: billing::BillingRecordQuery,
    account_user: Option<String>,
) -> Result<billing::BillingRecordResult, String> {
    let (account, compatibility) = billing_action_target(&state, account_user.as_deref())?;
    let kind = query.kind.clone();
    let page = query.page;
    let all = query.all;
    let _fetch_guard = state.billing_fetch_lock.lock().await;
    ensure_billing_foreground(&state)?;
    let result = run_billing_read_while_foreground(&state, async {
        billing::query_records(&account.user, &account.pass, compatibility, &query)
            .await
            .map_err(|error| error.user_message())
    })
    .await;
    match &result {
        Ok(data) => rust_log(
            &app,
            &state,
            "计费",
            &format!(
                "计费记录已读取（类型={kind}，页码={}，返回={}，总数={}）",
                if all { 0 } else { page },
                data.table.rows.len(),
                data.table.total
            ),
            "debug",
        ),
        Err(error) => rust_log(&app, &state, "计费", error, "error"),
    }
    result
}

#[tauri::command]
async fn perform_billing_action(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    request: billing::BillingActionRequest,
    account_user: Option<String>,
) -> Result<billing::BillingActionResult, String> {
    let action = request.action.clone();
    let (account, compatibility) = if action == "changePassword" {
        (
            campus_service_target(&state, account_user.as_deref())?,
            None,
        )
    } else {
        let (account, compatibility) = billing_action_target(&state, account_user.as_deref())?;
        (account, Some(compatibility))
    };
    let new_password = request.new_password.clone();
    let mut result = if action == "changePassword" {
        ensure_billing_foreground(&state)?;
        let supplied_password = request
            .old_password
            .as_deref()
            .ok_or_else(|| "请输入当前统一认证密码".to_string())?;
        let replacement = request
            .new_password
            .as_deref()
            .ok_or_else(|| "请输入新统一认证密码".to_string())?;
        let _campus_guard = state.campus_service_lock.lock().await;
        ensure_billing_foreground(&state)?;
        rust_log(&app, &state, "计费", "正在通过统一认证修改密码", "info");
        let changed = campus_services::change_password(
            &account.user,
            &account.pass,
            supplied_password,
            replacement,
        )
        .await
        .map_err(campus_services::CampusServiceError::user_message);
        if let Err(error) = &changed {
            rust_log(&app, &state, "计费", error, "error");
        }
        let message = changed?;
        billing::BillingActionResult {
            message,
            password_changed: true,
        }
    } else {
        let _fetch_guard = state.billing_fetch_lock.lock().await;
        let compatibility = compatibility.ok_or_else(|| "计费操作缺少 VPN 兼容配置".to_string())?;
        ensure_billing_foreground(&state)?;
        run_billing_mutation_to_completion(&state, async {
            billing::perform_action(&account.user, &account.pass, compatibility, &request)
                .await
                .map_err(|error| error.user_message())
        })
        .await?
    };

    if result.password_changed {
        let replacement = new_password
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "统一认证密码已修改，但新密码没有返回到安全存储流程".to_string())?;
        let mut config = state.config.read().unwrap().clone();
        let saved = config
            .accounts
            .iter_mut()
            .find(|saved| saved.user == account.user)
            .ok_or_else(|| {
                "统一认证密码已修改，但 App 中已找不到对应账号，请立即重新添加账号".to_string()
            })?;
        saved.pass = replacement;
        save_config(&app, &state, config).map_err(|error| {
            format!(
                "统一认证密码已修改，但 App 安全存储同步失败：{error}。请立即在账号管理中更新密码"
            )
        })?;
        result.message = format!("{}；App 中的账号密码已同步更新", result.message);
    }

    let action_label = match action.as_str() {
        "stopNow" => "立即停机",
        "reopenNow" => "立即复通",
        "schedulePackage" => "预约套餐",
        "cancelPackage" => "取消套餐预约",
        "setConsumeLimit" => "修改消费保护",
        "bindMac" => "绑定设备",
        "unbindMac" => "解绑设备",
        "changePassword" => "修改统一认证密码",
        "updateQuestions" => "修改密码保护",
        _ => "计费操作",
    };
    rust_log(
        &app,
        &state,
        "计费",
        &format!("用户已确认执行：{action_label}"),
        "success",
    );
    Ok(result)
}

fn campus_service_target(state: &AppState, account_user: Option<&str>) -> Result<Account, String> {
    let config = state.config.read().unwrap();
    let account = selected_billing_account(&config, account_user).ok_or_else(|| {
        if account_user.is_some() {
            "所选统一认证账号不存在、已停用或缺少有效配置".to_string()
        } else {
            "没有可用于统一认证的已启用账号".to_string()
        }
    })?;
    if account.pass.is_empty() {
        return Err("统一认证账号缺少已保存的密码".to_string());
    }
    Ok(account)
}

fn campus_service_session_seed(
    state: &AppState,
    account: &str,
) -> Option<campus_services::PersistedCampusSession> {
    state
        .config
        .read()
        .unwrap()
        .campus_service_sessions
        .iter()
        .find(|session| session.account() == account)
        .cloned()
}

fn persist_campus_service_session(
    app: &tauri::AppHandle,
    state: &AppState,
    session: campus_services::PersistedCampusSession,
) {
    let snapshot = {
        let mut config = state.config.write().unwrap();
        config
            .campus_service_sessions
            .retain(|current| current.account() != session.account());
        config.campus_service_sessions.push(session);
        config.clone()
    };
    if let Err(error) = save_secure_config_verified(app, &snapshot) {
        rust_log(
            app,
            state,
            "计费",
            &format!("移动门户登录状态未能写入安全存储：{error}"),
            "error",
        );
    } else {
        rust_log(
            app,
            state,
            "计费",
            "已在安全存储中更新移动门户长期登录状态",
            "debug",
        );
    }
}

fn persist_recharge_transaction(
    app: &tauri::AppHandle,
    state: &AppState,
    transaction: recharge_state::RechargeTransaction,
) -> Result<(), String> {
    let snapshot = {
        let mut config = state.config.write().unwrap();
        config.recharge_transactions.upsert(transaction);
        config.clone()
    };
    save_recharge_snapshot_verified(app, &snapshot)
}

fn save_recharge_snapshot_verified(
    app: &tauri::AppHandle,
    snapshot: &AppConfig,
) -> Result<(), String> {
    let mut last_error = String::new();
    for attempt in 0..3 {
        match save_secure_config_verified(app, snapshot) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = error,
        }
        if attempt < 2 {
            std::thread::sleep(std::time::Duration::from_millis(40 * (attempt + 1)));
        }
    }
    Err(format!("充值恢复记录连续写入失败：{last_error}"))
}

fn transition_recharge_transaction(
    app: &tauri::AppHandle,
    state: &AppState,
    id: &str,
    stage: recharge_state::RechargeStage,
    note: impl Into<String>,
) -> Result<(), String> {
    let snapshot = {
        let mut config = state.config.write().unwrap();
        config
            .recharge_transactions
            .transition(id, stage, chrono::Utc::now().timestamp(), note)?;
        config.clone()
    };
    save_recharge_snapshot_verified(app, &snapshot)
}

fn transition_recharge_transaction_with_parent(
    app: &tauri::AppHandle,
    state: &AppState,
    id: &str,
    stage: recharge_state::RechargeStage,
    note: impl Into<String>,
) -> Result<(), String> {
    let snapshot = {
        let mut config = state.config.write().unwrap();
        config.recharge_transactions.transition_with_parent(
            id,
            stage,
            chrono::Utc::now().timestamp(),
            note,
        )?;
        config.clone()
    };
    save_recharge_snapshot_verified(app, &snapshot)
}

#[tauri::command]
fn get_recoverable_recharges(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
) -> Vec<recharge_state::RechargeRecoveryView> {
    let (views, reconciled_snapshot) = {
        let mut config = state.config.write().unwrap();
        let changed = config
            .recharge_transactions
            .reconcile_legacy_completed_transfers();
        let views = config.recharge_transactions.recovery_views();
        (views, changed.then(|| config.clone()))
    };
    if let Some(snapshot) = reconciled_snapshot {
        if let Err(error) = save_recharge_snapshot_verified(&app, &snapshot) {
            rust_log(
                &app,
                &state,
                "计费",
                &format!("旧版充值恢复状态已在内存修复，但持久化失败：{error}"),
                "error",
            );
        }
    }
    views
}

#[tauri::command]
fn finish_recharge_recovery(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    id: String,
    completed: bool,
    note: Option<String>,
) -> Result<(), String> {
    transition_recharge_transaction(
        &app,
        &state,
        &id,
        if completed {
            recharge_state::RechargeStage::Completed
        } else {
            recharge_state::RechargeStage::Unknown
        },
        note.unwrap_or_else(|| {
            if completed {
                "充值流程已完成".to_string()
            } else {
                "充值结果仍需核对".to_string()
            }
        }),
    )
}

fn campus_recharge_open_at_hour(hour: u64) -> bool {
    (6..23).contains(&hour)
}

fn ensure_campus_recharge_open() -> Result<(), String> {
    let unix_seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let beijing_hour = (unix_seconds / 3600 + 8) % 24;
    if campus_recharge_open_at_hour(beijing_hour) {
        Ok(())
    } else {
        Err(
            "充值系统仅在北京时间每日 06:00–23:00 开放；当前可查看余额，但不能创建或确认充值订单"
                .to_string(),
        )
    }
}

#[tauri::command]
async fn prepare_network_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    target_account: String,
    amount: String,
    account_user: Option<String>,
    recovery_source_id: Option<String>,
) -> Result<campus_services::RechargePreview, String> {
    ensure_campus_recharge_open()?;
    let account = campus_service_target(&state, account_user.as_deref())?;
    let _service_guard = state.campus_service_lock.lock().await;
    let session_seed = campus_service_session_seed(&state, &account.user);
    rust_log(
        &app,
        &state,
        "计费",
        "正在通过统一认证核对校园卡与目标网费账户",
        "info",
    );
    #[cfg(target_os = "android")]
    {
        let (transport, validated) = {
            let network = state.last_network_state.lock().unwrap();
            (
                network
                    .get("transport")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
                network
                    .get("validated")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
            )
        };
        rust_log(
            &app,
            &state,
            "计费",
            &format!("统一认证网络上下文: transport={transport}, systemValidated={validated}"),
            "debug",
        );
    }
    let prepared = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        campus_services::prepare_recharge(
            &account.user,
            &account.pass,
            &target_account,
            &amount,
            session_seed,
        ),
    )
    .await
    .map_err(|_| "充值信息核对超过 60 秒，已取消本次请求".to_string())?
    .map_err(campus_services::CampusServiceError::user_message);
    match prepared {
        Ok((pending, preview, persisted)) => {
            persist_campus_service_session(&app, &state, persisted);
            let mut transaction = recharge_state::RechargeTransaction::prepared(
                preview.confirmation_id.clone(),
                "campusCard",
                preview.payer_account.clone(),
                preview.target_account.clone(),
                preview.amount.clone(),
                preview.card_balance.clone(),
                chrono::Utc::now().timestamp(),
            );
            let snapshot = {
                let mut config = state.config.write().unwrap();
                if let Some(source_id) = recovery_source_id.as_deref() {
                    let source = config
                        .recharge_transactions
                        .find(source_id)
                        .cloned()
                        .ok_or_else(|| {
                            "找不到对应的支付恢复记录，请重新打开充值页面".to_string()
                        })?;
                    let source_amount = source.amount.parse::<f64>().unwrap_or(f64::NAN);
                    let transfer_amount = preview.amount.parse::<f64>().unwrap_or(f64::NAN);
                    if !matches!(source.method.as_str(), "alipay" | "wechat")
                        || !source.stage.is_recoverable()
                        || source.payer_account != preview.payer_account
                        || source.target_account != preview.target_account
                        || !source_amount.is_finite()
                        || !transfer_amount.is_finite()
                        || (source_amount - transfer_amount).abs() >= 0.005
                    {
                        return Err("支付恢复记录与本次网费转入信息不一致，已停止操作".to_string());
                    }
                    transaction.parent_id = source_id.to_string();
                    config.recharge_transactions.transition(
                        source_id,
                        recharge_state::RechargeStage::PaymentConfirmed,
                        chrono::Utc::now().timestamp(),
                        "已确认支付到账，等待转入目标网费账户",
                    )?;
                }
                config.recharge_transactions.upsert(transaction);
                config.clone()
            };
            save_recharge_snapshot_verified(&app, &snapshot)?;
            *state.campus_recharge_pending.lock().await = Some(pending);
            rust_log(
                &app,
                &state,
                "计费",
                "校园卡与目标网费账户核对完成，等待用户确认",
                "debug",
            );
            Ok(preview)
        }
        Err(error) => {
            *state.campus_recharge_pending.lock().await = None;
            rust_log(&app, &state, "计费", &error, "error");
            Err(error)
        }
    }
}

#[tauri::command]
async fn confirm_network_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    confirmation_id: String,
) -> Result<campus_services::RechargeResult, String> {
    ensure_campus_recharge_open()?;
    ensure_billing_foreground(&state)?;
    let _service_guard = state.campus_service_lock.lock().await;
    ensure_billing_foreground(&state)?;
    let pending = {
        let mut slot = state.campus_recharge_pending.lock().await;
        let current = slot
            .as_ref()
            .ok_or_else(|| "没有待确认的充值，请先重新核对账户与金额".to_string())?;
        if current.confirmation_id() != confirmation_id {
            return Err("充值确认标识不匹配，请重新核对账户与金额".to_string());
        }
        if current.expired() {
            *slot = None;
            return Err("充值确认已过期，请重新核对账户与金额".to_string());
        }
        slot.take()
            .ok_or_else(|| "待确认的充值状态已失效，请重新核对账户与金额".to_string())?
    };
    rust_log(
        &app,
        &state,
        "计费",
        "用户已二次确认校园卡网费充值，正在提交一次性订单",
        "info",
    );
    transition_recharge_transaction_with_parent(
        &app,
        &state,
        &confirmation_id,
        recharge_state::RechargeStage::TransferSubmitted,
        "已提交校园卡到网费账户的写操作",
    )?;
    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(45),
        campus_services::execute_recharge(pending),
    )
    .await
    {
        Ok(result) => result.map_err(campus_services::CampusServiceError::user_message),
        Err(_) => {
            let message =
                "充值提交超过 45 秒，结果未知；请先查询校园卡和网费记录，不要立即重复充值"
                    .to_string();
            let _ = transition_recharge_transaction_with_parent(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Unknown,
                &message,
            );
            return Err(message);
        }
    };
    match &result {
        Ok(_) => {
            let _ = transition_recharge_transaction_with_parent(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Completed,
                "校园卡网费充值成功",
            );
            rust_log(&app, &state, "计费", "校园卡网费充值成功", "success")
        }
        Err(error) => {
            let _ = transition_recharge_transaction_with_parent(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Unknown,
                error,
            );
            rust_log(&app, &state, "计费", error, "error")
        }
    }
    result
}

#[tauri::command]
async fn get_network_recharge_balances(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    target_account: String,
    account_user: Option<String>,
) -> Result<campus_services::RechargeBalanceSnapshot, String> {
    let account = campus_service_target(&state, account_user.as_deref())?;
    let _service_guard = state.campus_service_lock.lock().await;
    let session_seed = campus_service_session_seed(&state, &account.user);
    rust_log(
        &app,
        &state,
        "计费",
        "正在刷新校园卡余额与目标网费余额",
        "debug",
    );
    let snapshot = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        campus_services::query_recharge_balances(
            &account.user,
            &account.pass,
            &target_account,
            session_seed,
        ),
    )
    .await
    .map_err(|_| "余额刷新超过 60 秒，已停止等待".to_string())?
    .map_err(campus_services::CampusServiceError::user_message);
    match snapshot {
        Ok((snapshot, persisted)) => {
            persist_campus_service_session(&app, &state, persisted);
            rust_log(&app, &state, "计费", "充值后余额刷新完成", "success");
            Ok(snapshot)
        }
        Err(error) => {
            rust_log(&app, &state, "计费", &error, "error");
            Err(error)
        }
    }
}

#[tauri::command]
async fn cancel_network_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    confirmation_id: String,
) -> Result<(), String> {
    let mut slot = state.campus_recharge_pending.lock().await;
    if slot
        .as_ref()
        .is_some_and(|pending| pending.confirmation_id() == confirmation_id)
    {
        *slot = None;
        let _ = transition_recharge_transaction(
            &app,
            &state,
            &confirmation_id,
            recharge_state::RechargeStage::Cancelled,
            "用户在提交前取消",
        );
    }
    Ok(())
}

#[tauri::command]
async fn prepare_alipay_card_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    target_account: String,
    amount: String,
    account_user: Option<String>,
) -> Result<campus_services::AlipayRechargePreview, String> {
    ensure_campus_recharge_open()?;
    let account = campus_service_target(&state, account_user.as_deref())?;
    let _service_guard = state.campus_service_lock.lock().await;
    let session_seed = campus_service_session_seed(&state, &account.user);
    rust_log(
        &app,
        &state,
        "计费",
        "正在通过统一认证核对支付宝充值所用校园卡",
        "info",
    );
    let prepared = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        campus_services::prepare_alipay_card_recharge(
            &account.user,
            &account.pass,
            &amount,
            session_seed,
        ),
    )
    .await
    .map_err(|_| "支付宝充值信息核对超过 60 秒，已取消本次请求".to_string())?
    .map_err(campus_services::CampusServiceError::user_message);
    match prepared {
        Ok((pending, preview, persisted)) => {
            persist_campus_service_session(&app, &state, persisted);
            persist_recharge_transaction(
                &app,
                &state,
                recharge_state::RechargeTransaction::prepared(
                    preview.confirmation_id.clone(),
                    "alipay",
                    preview.payer_account.clone(),
                    target_account.clone(),
                    preview.amount.clone(),
                    preview.card_balance.clone(),
                    chrono::Utc::now().timestamp(),
                ),
            )?;
            *state.campus_alipay_recharge_pending.lock().await = Some(pending);
            rust_log(
                &app,
                &state,
                "计费",
                "支付宝充值校园卡信息核对完成，等待用户确认",
                "debug",
            );
            Ok(preview)
        }
        Err(error) => {
            *state.campus_alipay_recharge_pending.lock().await = None;
            rust_log(&app, &state, "计费", &error, "error");
            Err(error)
        }
    }
}

#[tauri::command]
async fn confirm_alipay_card_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    confirmation_id: String,
) -> Result<campus_services::AlipayRechargeResult, String> {
    ensure_campus_recharge_open()?;
    ensure_billing_foreground(&state)?;
    let _service_guard = state.campus_service_lock.lock().await;
    ensure_billing_foreground(&state)?;
    let pending = {
        let mut slot = state.campus_alipay_recharge_pending.lock().await;
        let current = slot
            .as_ref()
            .ok_or_else(|| "没有待确认的支付宝充值，请先重新核对校园卡与金额".to_string())?;
        if current.confirmation_id() != confirmation_id {
            return Err("支付宝充值确认标识不匹配，请重新核对校园卡与金额".to_string());
        }
        if current.expired() {
            *slot = None;
            return Err("支付宝充值确认已过期，请重新核对校园卡与金额".to_string());
        }
        slot.take()
            .ok_or_else(|| "待确认的支付宝充值状态已失效，请重新核对".to_string())?
    };
    rust_log(
        &app,
        &state,
        "计费",
        "用户已二次确认支付宝充值校园卡，正在创建一次性支付订单",
        "info",
    );
    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(45),
        campus_services::execute_alipay_card_recharge(pending),
    )
    .await
    {
        Ok(result) => result.map_err(campus_services::CampusServiceError::user_message),
        Err(_) => {
            let message =
                "支付宝订单创建超过 45 秒，结果未知；请先检查校园卡充值记录，不要立即重复操作"
                    .to_string();
            let _ = transition_recharge_transaction(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Unknown,
                &message,
            );
            return Err(message);
        }
    };
    match &result {
        Ok(payment) => {
            let snapshot = {
                let mut config = state.config.write().unwrap();
                if let Some(transaction) = config
                    .recharge_transactions
                    .0
                    .iter_mut()
                    .find(|item| item.id == confirmation_id)
                {
                    transaction.stage = recharge_state::RechargeStage::HandedOff;
                    transaction.payment_url = payment.payment_url.clone();
                    transaction.updated_at = chrono::Utc::now().timestamp();
                    transaction.note = "支付宝订单已创建，等待付款".to_string();
                }
                config.clone()
            };
            if let Err(error) = save_recharge_snapshot_verified(&app, &snapshot) {
                let message = format!(
                    "支付宝订单可能已经创建，但恢复记录未能安全保存（{error}）。本次不会打开支付入口；请勿重复创建订单，先刷新充值恢复状态"
                );
                rust_log(&app, &state, "计费", &message, "error");
                return Err(message);
            }
            rust_log(&app, &state, "计费", "支付宝支付入口已安全生成", "success")
        }
        Err(error) => {
            let _ = transition_recharge_transaction(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Unknown,
                error,
            );
            rust_log(&app, &state, "计费", error, "error")
        }
    }
    result
}

#[tauri::command]
async fn cancel_alipay_card_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    confirmation_id: String,
) -> Result<(), String> {
    let mut slot = state.campus_alipay_recharge_pending.lock().await;
    if slot
        .as_ref()
        .is_some_and(|pending| pending.confirmation_id() == confirmation_id)
    {
        *slot = None;
        let _ = transition_recharge_transaction(
            &app,
            &state,
            &confirmation_id,
            recharge_state::RechargeStage::Cancelled,
            "用户在创建支付宝订单前取消",
        );
    }
    Ok(())
}

#[tauri::command]
async fn prepare_wechat_card_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    target_account: String,
    amount: String,
    account_user: Option<String>,
) -> Result<campus_services::WechatRechargePreview, String> {
    ensure_campus_recharge_open()?;
    let account = campus_service_target(&state, account_user.as_deref())?;
    let _service_guard = state.campus_service_lock.lock().await;
    let session_seed = campus_service_session_seed(&state, &account.user);
    rust_log(&app, &state, "计费", "正在核对微信充值所用校园卡", "info");
    let prepared = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        campus_services::prepare_wechat_card_recharge(
            &account.user,
            &account.pass,
            &target_account,
            &amount,
            session_seed,
        ),
    )
    .await
    .map_err(|_| "微信充值信息核对超过 60 秒，已取消本次请求".to_string())?
    .map_err(campus_services::CampusServiceError::user_message);
    match prepared {
        Ok((pending, preview, persisted)) => {
            persist_campus_service_session(&app, &state, persisted);
            persist_recharge_transaction(
                &app,
                &state,
                recharge_state::RechargeTransaction::prepared(
                    preview.confirmation_id.clone(),
                    "wechat",
                    preview.payer_account.clone(),
                    preview.target_account.clone(),
                    preview.amount.clone(),
                    preview.card_balance.clone(),
                    chrono::Utc::now().timestamp(),
                ),
            )?;
            *state.campus_wechat_recharge_pending.lock().await = Some(pending);
            rust_log(
                &app,
                &state,
                "计费",
                "微信充值校园卡信息核对完成，等待用户确认",
                "debug",
            );
            Ok(preview)
        }
        Err(error) => {
            *state.campus_wechat_recharge_pending.lock().await = None;
            rust_log(&app, &state, "计费", &error, "error");
            Err(error)
        }
    }
}

#[tauri::command]
async fn confirm_wechat_card_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    confirmation_id: String,
) -> Result<campus_services::WechatRechargeResult, String> {
    ensure_campus_recharge_open()?;
    ensure_billing_foreground(&state)?;
    let _service_guard = state.campus_service_lock.lock().await;
    ensure_billing_foreground(&state)?;
    let pending = {
        let mut slot = state.campus_wechat_recharge_pending.lock().await;
        let current = slot
            .as_ref()
            .ok_or_else(|| "没有待确认的微信充值，请先重新核对校园卡与金额".to_string())?;
        if current.confirmation_id() != confirmation_id {
            return Err("微信充值确认标识不匹配，请重新核对校园卡与金额".to_string());
        }
        if current.expired() {
            *slot = None;
            return Err("微信充值确认已过期，请重新核对校园卡与金额".to_string());
        }
        slot.take()
            .ok_or_else(|| "待确认的微信充值状态已失效，请重新核对".to_string())?
    };
    rust_log(
        &app,
        &state,
        "计费",
        "用户已确认微信充值校园卡，正在创建一次性支付订单",
        "info",
    );
    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(45),
        campus_services::execute_wechat_card_recharge(pending),
    )
    .await
    {
        Ok(result) => result.map_err(campus_services::CampusServiceError::user_message),
        Err(_) => {
            let message =
                "微信订单创建超过 45 秒，结果未知；请先检查校园卡充值记录，不要立即重复操作"
                    .to_string();
            let _ = transition_recharge_transaction(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Unknown,
                &message,
            );
            return Err(message);
        }
    };
    match result {
        Ok((payment, result)) => {
            let (payer, amount, openid, partner_jour_no) = payment.recovery_fields();
            let snapshot = {
                let mut config = state.config.write().unwrap();
                if let Some(transaction) = config
                    .recharge_transactions
                    .0
                    .iter_mut()
                    .find(|item| item.id == confirmation_id)
                {
                    transaction.stage = recharge_state::RechargeStage::HandedOff;
                    transaction.payment_id = result.payment_id.clone();
                    transaction.payment_url = result.launch_url.clone();
                    transaction.payer_account = payer.to_string();
                    transaction.amount = amount.to_string();
                    transaction.openid = openid.to_string();
                    transaction.partner_jour_no = partner_jour_no.to_string();
                    transaction.updated_at = chrono::Utc::now().timestamp();
                    transaction.note = "微信订单已创建，等待付款".to_string();
                }
                config.clone()
            };
            *state.campus_wechat_payment_pending.lock().await = Some(payment);
            if let Err(error) = save_recharge_snapshot_verified(&app, &snapshot) {
                let message = format!(
                    "微信订单可能已经创建，但恢复记录未能安全保存（{error}）。本次不会唤起微信；请勿重复创建订单，先刷新充值恢复状态"
                );
                rust_log(&app, &state, "计费", &message, "error");
                return Err(message);
            }
            rust_log(
                &app,
                &state,
                "计费",
                "Tenpay 会话已续接并取得受信任的微信唤起地址",
                "success",
            );
            Ok(result)
        }
        Err(error) => {
            let _ = transition_recharge_transaction(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Unknown,
                &error,
            );
            rust_log(&app, &state, "计费", &error, "error");
            Err(error)
        }
    }
}

#[tauri::command]
async fn check_wechat_card_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    payment_id: String,
) -> Result<campus_services::WechatPaymentStatus, String> {
    let _service_guard = state.campus_service_lock.lock().await;
    let needs_restore = {
        let slot = state.campus_wechat_payment_pending.lock().await;
        slot.as_ref()
            .is_none_or(|pending| pending.payment_id() != payment_id || pending.expired())
    };
    if needs_restore {
        let transaction = state
            .config
            .read()
            .unwrap()
            .recharge_transactions
            .find(&payment_id)
            .cloned()
            .ok_or_else(|| "没有可恢复的微信支付订单".to_string())?;
        if transaction.method != "wechat" || !transaction.stage.is_recoverable() {
            return Err("微信支付订单已结束或不可恢复".to_string());
        }
        let account = campus_service_target(&state, Some(&transaction.payer_account))?;
        let session_seed = campus_service_session_seed(&state, &account.user);
        let (pending, persisted) = campus_services::restore_wechat_card_recharge(
            &transaction.payment_id,
            &account.user,
            &account.pass,
            &transaction.amount,
            &transaction.partner_jour_no,
            session_seed,
        )
        .await
        .map_err(campus_services::CampusServiceError::user_message)?;
        persist_campus_service_session(&app, &state, persisted);
        *state.campus_wechat_payment_pending.lock().await = Some(pending);
    }
    let mut slot = state.campus_wechat_payment_pending.lock().await;
    let pending = slot
        .as_mut()
        .ok_or_else(|| "微信支付恢复失败".to_string())?;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        campus_services::check_wechat_card_recharge(pending),
    )
    .await
    .map_err(|_| "微信支付状态查询超时，请稍后重试".to_string())?
    .map_err(campus_services::CampusServiceError::user_message);
    match &result {
        Ok(status) if status.status == "paid" => {
            let _ = transition_recharge_transaction(
                &app,
                &state,
                &payment_id,
                recharge_state::RechargeStage::PaymentConfirmed,
                "微信支付状态已确认成功",
            );
            rust_log(&app, &state, "计费", "微信支付状态已确认成功", "success")
        }
        Ok(_) => rust_log(&app, &state, "计费", "微信支付尚未完成", "debug"),
        Err(error) => rust_log(&app, &state, "计费", error, "error"),
    }
    result
}

#[tauri::command]
async fn cancel_wechat_card_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    confirmation_id: Option<String>,
    payment_id: Option<String>,
) -> Result<(), String> {
    if let Some(confirmation_id) = confirmation_id {
        let mut slot = state.campus_wechat_recharge_pending.lock().await;
        if slot
            .as_ref()
            .is_some_and(|pending| pending.confirmation_id() == confirmation_id)
        {
            *slot = None;
            let _ = transition_recharge_transaction(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Cancelled,
                "用户在创建微信订单前取消",
            );
        }
    }
    if let Some(payment_id) = payment_id {
        let mut slot = state.campus_wechat_payment_pending.lock().await;
        if slot
            .as_ref()
            .is_some_and(|pending| pending.payment_id() == payment_id)
        {
            *slot = None;
        }
        let stage = state
            .config
            .read()
            .unwrap()
            .recharge_transactions
            .find(&payment_id)
            .map(|transaction| transaction.stage.clone());
        let transition = stage.and_then(recharge_state::stage_after_payment_context_closed);
        if let Some((next, note)) = transition {
            transition_recharge_transaction(&app, &state, &payment_id, next, note)?;
        }
    }
    Ok(())
}

fn billing_action_target(
    state: &AppState,
    account_user: Option<&str>,
) -> Result<(Account, VpnCompatibility), String> {
    ensure_billing_foreground(state)?;
    if is_mobile_data_network(&state.last_network_state.lock().unwrap()) {
        return Err("Android 移动数据网络下不能访问校园网计费系统".to_string());
    }
    let config = state.config.read().unwrap();
    let account = selected_billing_account(&config, account_user).ok_or_else(|| {
        if account_user.is_some() {
            "所选计费账号不存在、已停用或缺少有效配置".to_string()
        } else {
            "没有可用于计费系统的已启用账号".to_string()
        }
    })?;
    if account.pass.is_empty() {
        return Err("计费账号缺少已保存的密码".to_string());
    }
    Ok((account, effective_vpn_compatibility(&config)))
}

#[tauri::command]
async fn disconnect_billing_session(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    session_id: String,
    ip: String,
    mac: String,
    account_user: Option<String>,
) -> Result<String, String> {
    let (account, compatibility) = billing_action_target(&state, account_user.as_deref())?;
    let _fetch_guard = state.billing_fetch_lock.lock().await;
    ensure_billing_foreground(&state)?;
    let result = run_billing_mutation_to_completion(&state, async {
        billing::disconnect_session(
            &account.user,
            &account.pass,
            compatibility,
            &session_id,
            &ip,
            &mac,
        )
        .await
        .map_err(|error| error.user_message())
    })
    .await;
    match &result {
        Ok(_) => rust_log(
            &app,
            &state,
            "计费",
            "用户已确认注销一条在线会话",
            "success",
        ),
        Err(error) => rust_log(&app, &state, "计费", error, "error"),
    }
    result
}

#[tauri::command]
async fn set_billing_mauth(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    enabled: bool,
    account_user: Option<String>,
) -> Result<String, String> {
    let (account, compatibility) = billing_action_target(&state, account_user.as_deref())?;
    let _fetch_guard = state.billing_fetch_lock.lock().await;
    ensure_billing_foreground(&state)?;
    let result = run_billing_mutation_to_completion(&state, async {
        billing::set_mauth_enabled(&account.user, &account.pass, compatibility, enabled)
            .await
            .map_err(|error| error.user_message())
    })
    .await;
    match &result {
        Ok(_) => rust_log(
            &app,
            &state,
            "计费",
            "用户已确认修改无感认证状态",
            "success",
        ),
        Err(error) => rust_log(&app, &state, "计费", error, "error"),
    }
    result
}

#[tauri::command]
fn notify_network_change(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    source: Option<String>,
) {
    state.is_suspended.store(false, Ordering::SeqCst);
    state.non_campus_count.store(0, Ordering::SeqCst);
    let is_bg = app_is_in_background(&app, &state);
    rust_log(
        &app,
        &state,
        "网络",
        &format!(
            "收到{}网络变化事件，{}",
            source.unwrap_or_else(|| "系统".to_string()),
            if is_bg {
                "当前处于后台，先仅检测连通性；IP 变化后再完整检测"
            } else {
                "立即执行完整检测"
            }
        ),
        "info",
    );
    let app_clone = app.clone();
    let state_clone = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        trigger_network_check(app_clone, state_clone, !is_bg).await;
    });
}

#[tauri::command]
fn set_auto_login_pause(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    minutes: i64,
) -> i64 {
    let until = if minutes <= 0 {
        0
    } else {
        chrono::Utc::now().timestamp() + minutes.saturating_mul(60)
    };
    state.auto_login_paused_until.store(until, Ordering::SeqCst);
    rust_log(
        &app,
        &state,
        "自动登录",
        if until == 0 {
            "已恢复自动登录"
        } else {
            "已暂停自动登录 1 小时"
        },
        "info",
    );
    #[cfg(desktop)]
    refresh_tray_menu(&app, &state);
    until
}

#[tauri::command]
fn log_from_js(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    module: String,
    message: String,
    log_type: String,
) {
    rust_log(&app, &state, &module, &message, &log_type);
}

#[tauri::command]
fn set_background_state(state: tauri::State<Arc<AppState>>, is_bg: bool) {
    state.is_in_background.store(is_bg, Ordering::SeqCst);
    let val = state.countdown.load(Ordering::SeqCst);
    let cfg = state.config.read().unwrap();
    let configured_interval = if is_bg {
        cfg.check_interval_bg
    } else {
        cfg.check_interval
    };
    let mobile_data = is_mobile_data_network(&state.last_network_state.lock().unwrap());
    let max_interval = if mobile_data {
        mobile_data_check_interval(configured_interval, is_bg)
    } else {
        configured_interval
    };
    if val > max_interval {
        state.countdown.store(max_interval, Ordering::SeqCst);
    }
}

#[tauri::command]
fn get_current_network_state(state: tauri::State<Arc<AppState>>) -> serde_json::Value {
    state.last_network_state.lock().unwrap().clone()
}

#[cfg(desktop)]
fn refresh_tray_menu(app: &tauri::AppHandle, state: &AppState) {
    use tauri::menu::{MenuBuilder, MenuItem};
    let network_state = state.last_network_state.lock().unwrap().clone();
    let state_name = match network_state.get("state").and_then(|value| value.as_str()) {
        Some("Online") => "已联网",
        Some("BjutCampus") => "校园网待认证",
        _ => "离线",
    };
    let paused =
        state.auto_login_paused_until.load(Ordering::SeqCst) > chrono::Utc::now().timestamp();
    let config = state.config.read().unwrap().clone();
    let status_item = match MenuItem::with_id(
        app,
        "status",
        format!("状态：{state_name}"),
        false,
        None::<&str>,
    ) {
        Ok(item) => item,
        Err(_) => return,
    };
    let mut builder = MenuBuilder::new(app)
        .item(&status_item)
        .separator()
        .text("show", "显示主窗口")
        .text("check", "立即检测网络")
        .text("login", "立即登录")
        .text(
            "pause",
            if paused {
                "恢复自动登录"
            } else {
                "暂停自动登录 1 小时"
            },
        )
        .separator();
    for (index, account) in config
        .accounts
        .iter()
        .enumerate()
        .filter(|(_, account)| !account.is_disabled.unwrap_or(false))
    {
        let marker = if account.is_default { "✓" } else { " " };
        builder = builder.text(
            format!("account:{index}"),
            format!("{marker} 首选账号：{}", account.user),
        );
    }
    let menu = match builder.separator().text("quit", "退出").build() {
        Ok(menu) => menu,
        Err(_) => return,
    };
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_menu(Some(menu));
        let _ = tray.set_tooltip(Some(format!("BJUT-AL · {state_name}")));
    }
}

#[cfg(desktop)]
async fn tray_manual_login(app: tauri::AppHandle, state: Arc<AppState>) {
    let network = get_network_info(app.clone(), Some(true));
    let transport = network_transport(&network).to_string();
    let ssid = network
        .get("ssid")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let bssid = network
        .get("bssid")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let ip = network
        .get("ip")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let config = state.config.read().unwrap().clone();
    let compatibility = effective_vpn_compatibility(&config);
    let detected = detect_login_type_rust(compatibility).await;
    let profile = matching_network_profile(&config, &ssid, &bssid, &detected);
    let login_type = profile
        .as_ref()
        .and_then(|item| login_type_from_profile(&item.login_type))
        .unwrap_or(detected);
    if login_type == LoginType::Unknown {
        let _ = show_native_notification(&app, "校园网登录", "未检测到可用的校园网认证网关");
        return;
    }
    if let Err(reason) = automatic_login_network_allowed(
        &login_type,
        &ssid,
        &bssid,
        &ip,
        &transport,
        &config.whitelist,
        &config.blacklist,
    ) {
        rust_log(
            &app,
            &state,
            "安全",
            &format!("托盘登录已阻止：{reason}"),
            "error",
        );
        let _ = show_native_notification(&app, "校园网登录已阻止", &reason);
        return;
    }
    let accounts = accounts_for_profile(config.accounts, profile.as_ref());
    let account = accounts
        .iter()
        .find(|account| account.is_default)
        .or_else(|| accounts.first());
    let Some(account) = account else {
        let _ = show_native_notification(&app, "校园网登录", "没有可用且已保存密码的账号");
        return;
    };
    if let Err(remaining) = account_attempt_allowed(&state, &account.user) {
        let _ = show_native_notification(
            &app,
            "账号正在冷却",
            &format!("请在 {remaining} 秒后重试或在诊断页解除"),
        );
        return;
    }
    rust_log(
        &app,
        &state,
        "托盘",
        &format!("使用首选账号 {} 执行快捷登录", account.user),
        "info",
    );
    match login_to_campus_network_rust(login_type, &account.user, &account.pass, compatibility)
        .await
    {
        Ok((true, message)) => {
            record_account_success(&app, &state, &account.user);
            rust_log(
                &app,
                &state,
                "托盘",
                &format!("快捷登录成功：{message}"),
                "success",
            );
            let _ = show_native_notification(
                &app,
                "校园网登录成功",
                &format!("账号：{}", account.user),
            );
            trigger_network_check(app, state, true).await;
        }
        Ok((false, message)) => {
            record_account_failure(&app, &state, &account.user, &message);
            rust_log(
                &app,
                &state,
                "托盘",
                &format!("快捷登录失败：{message}"),
                "error",
            );
            let _ = show_native_notification(&app, "校园网登录失败", &message);
        }
        Err(error) => {
            record_account_failure(&app, &state, &account.user, &format!("请求出错: {error}"));
            rust_log(
                &app,
                &state,
                "托盘",
                &format!("快捷登录出错：{error}"),
                "error",
            );
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().setup(|_app| {
        let app_state = std::sync::Arc::new(AppState {
            config: RwLock::new(AppConfig {
                accounts: Vec::new(),
                auto_login: false,
                check_interval: 15,
                check_interval_bg: 60,
                wifi_change_detect: true,
                log_level: "info".to_string(),
                vpn_compatibility: default_vpn_compatibility(),
                vpn_maximum_until: None,
                whitelist: Vec::new(),
                blacklist: Vec::new(),
                network_profiles: Vec::new(),
                usage_alerts: true,
                balance_alert_threshold: default_balance_alert_threshold(),
                flow_alert_threshold: default_flow_alert_threshold(),
                android_notification_mode: default_android_notification_mode(),
                android_notify_network_status: true,
                android_notify_login_results: true,
                android_notify_background_errors: true,
                campus_service_sessions: Vec::new(),
                recharge_transactions: recharge_state::RechargeJournal::default(),
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
            auto_login_paused_until: std::sync::atomic::AtomicI64::new(0),
            usage_alert_history: Mutex::new(HashMap::new()),
            billing_fetch_lock: tokio::sync::Mutex::new(()),
            campus_service_lock: tokio::sync::Mutex::new(()),
            campus_recharge_pending: tokio::sync::Mutex::new(None),
            campus_alipay_recharge_pending: tokio::sync::Mutex::new(None),
            campus_wechat_recharge_pending: tokio::sync::Mutex::new(None),
            campus_wechat_payment_pending: tokio::sync::Mutex::new(None),
            pending_discovered_account: tokio::sync::Mutex::new(None),
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
                    rust_log(
                        &loop_handle,
                        &loop_state,
                        "网络",
                        "执行等待中的完整网络检测",
                        "debug",
                    );
                    trigger_network_check(loop_handle.clone(), loop_state.clone(), true).await;
                    continue;
                }

                // 1. Local interface change check. Android NetworkCallback is the
                // primary signal on mobile data, so polling can be much slower there.
                wifi_check_counter += 1;
                let mobile_data_active =
                    is_mobile_data_network(&loop_state.last_network_state.lock().unwrap());
                let interface_poll_interval = if mobile_data_active { 15 } else { 3 };
                if wifi_check_counter >= interface_poll_interval {
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
                        rust_log(
                            &loop_handle,
                            &loop_state,
                            "网络",
                            &format!(
                                "[DEBUG] 执行网络接口变更检测。当前 IP: {} (上次 IP: {})",
                                current_ip,
                                last_ip.as_deref().unwrap_or("空")
                            ),
                            "debug",
                        );
                        if ip_changed {
                            rust_log(
                                &loop_handle,
                                &loop_state,
                                "网络",
                                &format!(
                                    "检测到局域网 IP 发生变更: {} -> {}，重新检测网络环境...",
                                    last_ip.unwrap_or_default(),
                                    current_ip
                                ),
                                "info",
                            );
                            loop_state.is_suspended.store(false, Ordering::SeqCst);
                            loop_state.non_campus_count.store(0, Ordering::SeqCst);
                            trigger_network_check(loop_handle.clone(), loop_state.clone(), true)
                                .await;
                            continue;
                        }
                    }
                }

                // 2. Connectivity Check Loop (every 1 second)
                if !is_chk {
                    if !is_bg && is_susp {
                        rust_log(
                            &loop_handle,
                            &loop_state,
                            "网络",
                            "检测到已返回前台，恢复连通性检测...",
                            "info",
                        );
                        loop_state.is_suspended.store(false, Ordering::SeqCst);
                        loop_state.non_campus_count.store(0, Ordering::SeqCst);
                        trigger_network_check(loop_handle.clone(), loop_state.clone(), true).await;
                        continue;
                    }
                    if is_susp {
                        let _ = loop_handle
                            .emit("countdown-tick", serde_json::json!({"status": "suspended"}));
                        continue;
                    }
                    let val = loop_state.countdown.fetch_sub(1, Ordering::SeqCst);
                    let current_countdown = val - 1;
                    if current_countdown <= 0 {
                        rust_log(
                            &loop_handle,
                            &loop_state,
                            "网络",
                            "[DEBUG] 倒计时归零，触发自动网络连通性检测",
                            "debug",
                        );
                        trigger_network_check(loop_handle.clone(), loop_state.clone(), !is_bg)
                            .await;
                    } else {
                        let _ = loop_handle.emit(
                            "countdown-tick",
                            serde_json::json!({
                                "status": "ticking",
                                "seconds": current_countdown
                            }),
                        );
                    }
                } else {
                    let _ = loop_handle
                        .emit("countdown-tick", serde_json::json!({"status": "checking"}));
                }
            }
        });

        let init_handle = app_handle.clone();
        let init_state = state_clone.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let full_details = !app_is_in_background(&init_handle, &init_state);
            trigger_network_check(init_handle, init_state, full_details).await;
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
                window.on_window_event(move |event| match event {
                    tauri::WindowEvent::Focused(focused) => {
                        window_state
                            .is_in_background
                            .store(!focused, Ordering::SeqCst);
                    }
                    tauri::WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        window_state.is_in_background.store(true, Ordering::SeqCst);
                        let _ = window_clone.hide();
                    }
                    _ => {}
                });
            }

            // System Tray Setup
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

            let mut tray_builder = TrayIconBuilder::with_id("main-tray");

            #[cfg(target_os = "macos")]
            {
                let mac_icon =
                    tauri::image::Image::from_bytes(include_bytes!("../icons/tray_mac.png"))
                        .expect("Failed to load macOS tray icon");
                tray_builder = tray_builder.icon(mac_icon);
            }
            #[cfg(not(target_os = "macos"))]
            {
                if let Some(ic) = _app.default_window_icon().cloned() {
                    tray_builder = tray_builder.icon(ic);
                }
            }

            let _tray = tray_builder
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id == "show" {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    } else if event.id == "check" {
                        let state = app.state::<Arc<AppState>>().inner().clone();
                        let app_handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            trigger_network_check(app_handle, state, true).await;
                        });
                    } else if event.id == "login" {
                        let state = app.state::<Arc<AppState>>().inner().clone();
                        let app_handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            tray_manual_login(app_handle, state).await;
                        });
                    } else if event.id == "pause" {
                        let state = app.state::<Arc<AppState>>();
                        let paused = state.auto_login_paused_until.load(Ordering::SeqCst)
                            > chrono::Utc::now().timestamp();
                        state.auto_login_paused_until.store(
                            if paused {
                                0
                            } else {
                                chrono::Utc::now().timestamp() + 3600
                            },
                            Ordering::SeqCst,
                        );
                        rust_log(
                            app,
                            &state,
                            "托盘",
                            if paused {
                                "已恢复自动登录"
                            } else {
                                "已暂停自动登录 1 小时"
                            },
                            "info",
                        );
                        refresh_tray_menu(app, &state);
                    } else if let Some(index) = event
                        .id
                        .as_ref()
                        .strip_prefix("account:")
                        .and_then(|value| value.parse::<usize>().ok())
                    {
                        let state = app.state::<Arc<AppState>>();
                        let mut config = state.config.read().unwrap().clone();
                        if index < config.accounts.len() {
                            for (account_index, account) in config.accounts.iter_mut().enumerate() {
                                account.is_default = account_index == index;
                            }
                            if save_config(app, &state, config).is_ok() {
                                rust_log(app, &state, "托盘", "已切换首选账号", "info");
                                let _ = app.emit(
                                    "preferred-account-change",
                                    serde_json::json!({ "index": index }),
                                );
                                refresh_tray_menu(app, &state);
                            }
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

            refresh_tray_menu(_app.handle(), &state_clone);

            #[cfg(target_os = "macos")]
            {
                let _ = _tray.set_icon_as_template(true);
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
        builder = builder
            .plugin(tauri_plugin_autostart::Builder::default().build())
            .plugin(tauri_plugin_updater::Builder::new().build());
    }

    let app = builder
        .invoke_handler(tauri::generate_handler![
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
            verify_legacy_credential_fingerprint,
            get_account_password,
            get_credential_storage_status,
            get_credential_storage_health,
            get_account_health,
            reset_account_health,
            run_network_diagnostics,
            create_diagnostic_bundle,
            get_logs,
            get_log_text,
            export_logs,
            export_billing_csv,
            clear_all_logs,
            get_countdown_status,
            trigger_manual_check,
            manual_login,
            get_user_info,
            discover_current_campus_account,
            accept_discovered_campus_account,
            reject_discovered_campus_account,
            get_billing_center,
            query_billing_records,
            perform_billing_action,
            prepare_network_recharge,
            confirm_network_recharge,
            get_recoverable_recharges,
            finish_recharge_recovery,
            get_network_recharge_balances,
            cancel_network_recharge,
            prepare_alipay_card_recharge,
            confirm_alipay_card_recharge,
            cancel_alipay_card_recharge,
            prepare_wechat_card_recharge,
            confirm_wechat_card_recharge,
            check_wechat_card_recharge,
            cancel_wechat_card_recharge,
            disconnect_billing_session,
            set_billing_mauth,
            notify_network_change,
            set_auto_login_pause,
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
    fn mobile_data_runtime_policy_is_android_only() {
        assert_eq!(
            is_mobile_data_network(&serde_json::json!({"transport": "cellular"})),
            cfg!(target_os = "android")
        );
        assert_eq!(
            is_mobile_data_network(&serde_json::json!({"transport": "CELLULAR"})),
            cfg!(target_os = "android")
        );
        assert!(!is_mobile_data_network(&serde_json::json!({
            "ssid": "<unknown ssid>",
            "bssid": "00:00:00:00:00:00",
            "ip": "0.0.0.0"
        })));
        assert!(!is_mobile_data_network(
            &serde_json::json!({"transport": "wifi"})
        ));
        assert_eq!(
            network_is_system_validated(&serde_json::json!({"validated": true})),
            cfg!(target_os = "android")
        );
    }

    #[test]
    fn mobile_data_detection_uses_battery_friendly_minimum_intervals() {
        assert_eq!(mobile_data_check_interval(15, false), 120);
        assert_eq!(mobile_data_check_interval(60, true), 300);
        assert_eq!(mobile_data_check_interval(600, false), 600);
        assert_eq!(mobile_data_check_interval(600, true), 600);
    }

    #[test]
    fn encodes_portal_query_values() {
        assert_eq!(
            url_encode("a b@北工大"),
            "a%20b%40%E5%8C%97%E5%B7%A5%E5%A4%A7"
        );
    }

    #[test]
    fn parses_success_and_failure_portal_responses() {
        assert_eq!(
            parse_dr_response("dr1003({\"result\":1})"),
            (true, "Portal协议认证成功！".to_string())
        );
        assert_eq!(
            parse_dr_response("dr1002({\"result\":0,\"msga\":\"密码错误\"})"),
            (false, "密码错误".to_string())
        );
        assert_eq!(
            parse_dr_response("not json"),
            (false, "解析响应数据失败".to_string())
        );
    }

    #[test]
    fn extracts_v6ip_from_both_html_quote_styles() {
        assert_eq!(
            find_v6ip("<input name=\"v6ip\" value=\"2001:db8::1\">"),
            "2001:db8::1"
        );
        assert_eq!(
            find_v6ip("<input name='v6ip' value='2001:db8::2'>"),
            "2001:db8::2"
        );
    }

    #[test]
    fn billing_account_selection_honors_requested_and_default_accounts() {
        let config: AppConfig = serde_json::from_value(serde_json::json!({
            "accounts": [
                {"user": "20260001", "pass": "first", "isDefault": true},
                {"user": "20260002", "pass": "second", "isDefault": false},
                {"user": "20260003", "pass": "disabled", "isDefault": false, "isDisabled": true}
            ]
        }))
        .unwrap();

        assert_eq!(
            selected_billing_account(&config, Some("20260002")).map(|account| account.user),
            Some("20260002".to_string())
        );
        assert_eq!(
            selected_billing_account(&config, None).map(|account| account.user),
            Some("20260001".to_string())
        );
        assert!(selected_billing_account(&config, Some("20260003")).is_none());
        assert!(selected_billing_account(&config, Some("missing")).is_none());
    }

    #[test]
    fn public_config_never_contains_passwords() {
        let persisted_session: campus_services::PersistedCampusSession =
            serde_json::from_value(serde_json::json!({
                "account": "20260001",
                "cookies": [{
                    "name": "eai-sess",
                    "value": "durable-cookie-secret",
                    "domain": "itsapp.bjut.edu.cn",
                    "host_only": true,
                    "path": "/",
                    "expires_at": 4102444800_i64,
                    "secure": true,
                    "http_only": true
                }],
                "saved_at": 1784707200_i64
            }))
            .unwrap();
        let config = AppConfig {
            accounts: vec![Account {
                user: "20260001".to_string(),
                pass: "secret".to_string(),
                is_default: true,
                is_disabled: None,
            }],
            auto_login: true,
            check_interval: 15,
            check_interval_bg: 60,
            wifi_change_detect: true,
            log_level: "info".to_string(),
            vpn_compatibility: default_vpn_compatibility(),
            vpn_maximum_until: None,
            whitelist: vec!["campus|trusted".to_string()],
            blacklist: vec!["guest|blocked".to_string()],
            network_profiles: vec![],
            usage_alerts: true,
            balance_alert_threshold: default_balance_alert_threshold(),
            flow_alert_threshold: default_flow_alert_threshold(),
            android_notification_mode: default_android_notification_mode(),
            android_notify_network_status: true,
            android_notify_login_results: true,
            android_notify_background_errors: true,
            campus_service_sessions: vec![persisted_session],
            recharge_transactions: recharge_state::RechargeJournal::default(),
        };
        let serialized = serde_json::to_string(&public_config(&config)).unwrap();
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("eai-sess"));
        assert!(!serialized.contains("durable-cookie-secret"));
        assert_eq!(public_config(&config).accounts[0].pass, "");
        assert!(public_config(&config).campus_service_sessions.is_empty());
        assert!(public_config(&config).whitelist.is_empty());
        assert!(public_config(&config).blacklist.is_empty());
        assert!(public_config(&config).recharge_transactions.0.is_empty());
    }

    #[test]
    fn credential_fingerprint_matches_frontend_canonical_json() {
        let config: AppConfig = serde_json::from_value(serde_json::json!({
            "accounts": [
                {"user": "25000001", "pass": "secret", "isDefault": true}
            ]
        }))
        .unwrap();
        assert_eq!(
            credential_snapshot_fingerprint(&config, &["25000001".to_string()]).as_deref(),
            Some("RIaN0amLX2hGe8y+AWFlPi5loCz5luI2i2rvaEyF2m4=")
        );
        assert!(credential_snapshot_fingerprint(&config, &["missing".to_string()]).is_none());
    }

    #[test]
    fn maximum_vpn_mode_expires_to_high_compatibility() {
        let mut config: AppConfig = serde_json::from_value(serde_json::json!({
            "accounts": [],
            "vpn_compatibility": "maximum",
            "vpn_maximum_until": chrono::Utc::now().timestamp() - 1
        }))
        .unwrap();
        assert_eq!(effective_vpn_compatibility(&config), VpnCompatibility::High);

        config.vpn_maximum_until = Some(chrono::Utc::now().timestamp() + 60);
        assert_eq!(
            effective_vpn_compatibility(&config),
            VpnCompatibility::Maximum
        );
    }

    #[test]
    fn legacy_config_uses_compatible_android_notification_defaults() {
        let config: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[],
            "autoLogin":true
        }"#,
        )
        .unwrap();

        assert_eq!(config.android_notification_mode, "combined");
        assert!(config.android_notify_network_status);
        assert!(config.android_notify_login_results);
        assert!(config.android_notify_background_errors);
    }

    #[test]
    fn migrates_passwords_from_legacy_plaintext_config() {
        let mut current: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[{"user":"20260001","pass":"","isDefault":true}]
        }"#,
        )
        .unwrap();
        let legacy: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[{"username":"20260001","password":"legacy-secret","is_default":true}],
            "autoLogin":true,
            "checkInterval":30
        }"#,
        )
        .unwrap();

        assert!(merge_legacy_credentials(&mut current, &legacy));
        assert_eq!(current.accounts[0].pass, "legacy-secret");
        assert!(legacy.auto_login);
        assert_eq!(legacy.check_interval, 30);
    }

    #[test]
    fn legacy_migration_never_overwrites_a_new_password() {
        let mut current: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[{"user":"20260001","pass":"new-secret","isDefault":true}]
        }"#,
        )
        .unwrap();
        let legacy: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[{"user":"20260001","pass":"old-secret","isDefault":true}]
        }"#,
        )
        .unwrap();

        assert!(!merge_legacy_credentials(&mut current, &legacy));
        assert_eq!(current.accounts[0].pass, "new-secret");
    }

    #[test]
    fn legacy_migration_keeps_current_values_and_adds_missing_accounts() {
        let mut current: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[{"user":"a","pass":"new-a","isDefault":true}]
        }"#,
        )
        .unwrap();
        let legacy: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[
                {"user":"a","pass":"old-a","isDefault":true},
                {"user":"b","pass":"old-b","isDefault":true}
            ]
        }"#,
        )
        .unwrap();

        assert!(merge_legacy_credentials(&mut current, &legacy));
        assert_eq!(current.accounts.len(), 2);
        assert_eq!(current.accounts[0].pass, "new-a");
        assert_eq!(current.accounts[1].user, "b");
        assert_eq!(current.accounts[1].pass, "old-b");
        assert!(!current.accounts[1].is_default);
    }

    #[test]
    fn blank_password_updates_reuse_existing_secrets() {
        let existing: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[{"user":"a","pass":"saved-secret","isDefault":true}]
        }"#,
        )
        .unwrap();
        let mut update: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[{"user":"a","pass":"","isDefault":true}]
        }"#,
        )
        .unwrap();

        assert!(fill_missing_passwords(&mut update, &existing));
        assert_eq!(update.accounts[0].pass, "saved-secret");
    }

    #[test]
    fn partial_password_repair_keeps_other_unrecoverable_accounts_editable() {
        let existing: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[
                {"user":"a","pass":"","isDefault":true},
                {"user":"b","pass":"","isDefault":false}
            ]
        }"#,
        )
        .unwrap();
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
                .use_rustls_tls()
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
            "wifi",
            &[],
            &[],
        )
        .is_ok());
        assert!(automatic_login_network_allowed(
            &LoginType::Type1,
            "evil-ap",
            "11:22",
            "10.21.2.3",
            "wifi",
            &[],
            &[],
        )
        .is_err());
        assert!(automatic_login_network_allowed(
            &LoginType::Type2,
            "custom-campus",
            "aa:bb",
            "10.21.2.3",
            "wifi",
            &whitelist,
            &[],
        )
        .is_ok());
        assert!(automatic_login_network_allowed(
            &LoginType::Type3,
            "",
            "",
            "192.168.1.5",
            "ethernet",
            &[],
            &[],
        )
        .is_err());
    }

    #[test]
    fn account_failures_use_bounded_cooldowns() {
        assert_eq!(
            classify_account_failure("密码错误", 1),
            ("credential", 1800)
        );
        assert_eq!(classify_account_failure("余额不足", 1), ("balance", 21600));
        assert_eq!(
            classify_account_failure("请求出错: timeout", 1),
            ("network", 15)
        );
        assert_eq!(
            classify_account_failure("请求出错: timeout", 20),
            ("network", 900)
        );
        assert_eq!(
            classify_account_failure("认证服务器繁忙", 1),
            ("server", 60)
        );
        assert_eq!(
            classify_account_failure("认证服务器繁忙", 20),
            ("server", 900)
        );
    }

    #[test]
    fn account_health_view_reports_active_cooldown() {
        let mut health = HashMap::new();
        health.insert(
            "student".to_string(),
            AccountHealth {
                consecutive_failures: 1,
                cooldown_until: Some(chrono::Utc::now().timestamp() + 120),
                failure_kind: Some("credential".to_string()),
                ..Default::default()
            },
        );

        let views = account_health_views(&health);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].status, "needs_attention");
        assert!(views[0].cooldown_seconds > 0);
    }

    #[test]
    fn network_profiles_match_exact_network_and_order_accounts() {
        let config: AppConfig = serde_json::from_str(
            r#"{
            "accounts": [
                {"user":"first","pass":"one","isDefault":true},
                {"user":"second","pass":"two","isDefault":false}
            ],
            "network_profiles": [{
                "id":"dorm","name":"宿舍","ssid":"bjut_sushe","login_type":"bjut-sushe",
                "account_order":["second"],"enabled":true
            }]
        }"#,
        )
        .unwrap();
        let profile = matching_network_profile(&config, "BJUT_SUSHE", "", &LoginType::Type1)
            .expect("profile should match case-insensitively");
        assert_eq!(
            login_type_from_profile(&profile.login_type),
            Some(LoginType::Type1)
        );
        let ordered = accounts_for_profile(config.accounts, Some(&profile));
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].user, "second");
    }

    #[test]
    fn network_profile_controls_auto_login_per_authentication_type() {
        let config: AppConfig = serde_json::from_str(
            r#"{
            "network_profiles": [{
                "id":"mixed","name":"自动识别","ssid":"bjut_wifi","enabled":true,
                "auto_login":false,
                "auto_login_types":{"type1":true,"type2":false,"type3":true}
            }]
        }"#,
        )
        .unwrap();
        let profile = &config.network_profiles[0];
        assert!(profile_auto_login_enabled(
            Some(profile),
            &LoginType::Type1,
            false
        ));
        assert!(!profile_auto_login_enabled(
            Some(profile),
            &LoginType::Type2,
            true
        ));
        assert!(profile_auto_login_enabled(
            Some(profile),
            &LoginType::Type3,
            false
        ));
        assert!(!profile_auto_login_enabled(
            Some(profile),
            &LoginType::Unknown,
            true
        ));
    }

    #[test]
    fn campus_profile_names_map_to_the_documented_gateways() {
        assert_eq!(
            login_type_from_profile("bjut-sushe"),
            Some(LoginType::Type1)
        );
        assert_eq!(
            login_type_from_profile("bjut_sushe"),
            Some(LoginType::Type1)
        );
        assert_eq!(login_type_from_profile("bjut-wifi"), Some(LoginType::Type2));
        assert_eq!(login_type_from_profile("bjut_wifi"), Some(LoginType::Type2));
        assert_eq!(login_type_from_profile("wired"), Some(LoginType::Type3));
    }

    #[test]
    fn vpn_compatibility_selects_secure_or_direct_probe_endpoints() {
        assert_eq!(
            portal_probe_urls(VpnCompatibility::Minimum, &LoginType::Type1),
            vec!["https://10.21.221.98:802/eportal/portal/login"]
        );
        assert_eq!(
            portal_probe_urls(VpnCompatibility::High, &LoginType::Type2),
            vec!["https://wlgn.bjut.edu.cn/drcom/login"]
        );
        assert_eq!(
            portal_probe_urls(VpnCompatibility::Maximum, &LoginType::Type2),
            vec!["http://10.21.251.3/drcom/login"]
        );
        assert_eq!(
            portal_probe_urls(VpnCompatibility::Maximum, &LoginType::Type3),
            vec!["http://172.30.201.2", "http://172.30.201.10"]
        );
    }

    #[test]
    fn dashboard_user_info_uses_the_lgn_https_host() {
        let url = reqwest::Url::parse(&lgn_user_info_url(1234)).unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("lgn.bjut.edu.cn"));
        assert_eq!(url.port(), Some(802));
        assert_eq!(
            url.query_pairs().find(|(key, _)| key == "v").unwrap().1,
            "1234"
        );
    }

    #[test]
    fn lgn_protocol_is_wired_only_unless_explicitly_trusted() {
        assert!(automatic_login_network_allowed(
            &LoginType::Type3,
            "bjut_wifi",
            "11:22",
            "10.21.2.3",
            "wifi",
            &[],
            &[],
        )
        .is_err());
        assert!(automatic_login_network_allowed(
            &LoginType::Type3,
            "",
            "",
            "10.21.2.3",
            "ethernet",
            &[],
            &[],
        )
        .is_ok());
        assert!(automatic_login_network_allowed(
            &LoginType::Type3,
            "",
            "",
            "10.21.2.3",
            "wifi",
            &[],
            &[],
        )
        .is_err());
    }

    #[test]
    fn usage_numbers_are_parsed_from_display_values() {
        assert_eq!(first_decimal("余额 9.50 元"), Some(9.5));
        assert_eq!(first_decimal("4.25 GB"), Some(4.25));
        assert_eq!(first_decimal("无限"), None);
    }

    #[test]
    fn campus_recharge_hours_use_beijing_half_open_range() {
        assert!(!campus_recharge_open_at_hour(5));
        assert!(campus_recharge_open_at_hour(6));
        assert!(campus_recharge_open_at_hour(22));
        assert!(!campus_recharge_open_at_hour(23));
    }
}
