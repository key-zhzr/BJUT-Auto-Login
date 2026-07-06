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
            
            if let Ok(client) = corewlan::WiFiClient::shared() {
                if let Some(interface) = client.interface() {
                    if let Some(s) = interface.ssid() {
                        ssid = s;
                    }
                    if let Some(b) = interface.bssid() {
                        bssid = b;
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
            let mut cmd = std::process::Command::new("netsh");
            cmd.args(["wlan", "show", "interfaces"]);
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            if let Ok(output) = cmd.output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let line = line.trim();
                    let line_upper = line.to_uppercase();
                    if line_upper.contains("BSSID") {
                        if let Some(idx) = line.find(':') {
                            bssid = line[idx + 1..].trim().to_string();
                        }
                    } else if line_upper.contains("SSID") {
                        if let Some(idx) = line.find(':') {
                            ssid = line[idx + 1..].trim().to_string();
                        }
                    }
                }
            }

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
fn get_local_ip() -> String {
    let mut ip = String::new();
    
    #[cfg(target_os = "windows")]
    {
        let mut ipconfig_ips = Vec::new();
        let mut cmd = std::process::Command::new("ipconfig");
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        if let Ok(output) = cmd.output() {
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
                        if let Ok(helper_class) = env.find_class("cn/edu/bjut/al/NetworkHelper") {
                            if let Ok(helper) = env.new_object(&helper_class, "()V", &[]) {
                                if let Ok(java_ip) = env.call_method(&helper, "getLocalIpAddress", "()Ljava/lang/String;", &[]) {
                                    if let Ok(ip_obj) = java_ip.l() {
                                        let ip_str: String = env.get_string(&jni::objects::JString::from(ip_obj)).unwrap().into();
                                        ip = ip_str;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            // Background network poller.
            // On desktop, it emits to frontend. On Android, it evaluates JS directly.
            let app_handle = _app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut was_connected = false;
                loop {
                    let mut is_connected = false;

                    #[cfg(target_os = "android")]
                    {
                        let info = get_network_info(app_handle.clone());
                        if let Some(ssid_val) = info.get("ssid").and_then(|s| s.as_str()) {
                            let s = ssid_val.replace("\"", "");
                            if s == "bjut_wifi" || s == "bjut_sushe" || s == "bjut-wifi" {
                                is_connected = true;
                            }
                        }
                    }

                    #[cfg(target_os = "macos")]
                    {
                        let info = get_network_info(app_handle.clone());
                        if let Some(ssid_val) = info.get("ssid").and_then(|s| s.as_str()) {
                            let s = ssid_val.replace("\"", "");
                            if s == "bjut_wifi" || s == "bjut_sushe" || s == "bjut-wifi" {
                                is_connected = true;
                            }
                        }
                        
                        if !is_connected {
                            if let Ok(output) = std::process::Command::new("ifconfig").output() {
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                if stdout.contains("inet 10.") {
                                    is_connected = true;
                                }
                            }
                        }
                    }

                    #[cfg(target_os = "windows")]
                    {
                        let info = get_network_info(app_handle.clone());
                        if let Some(ssid_val) = info.get("ssid").and_then(|s| s.as_str()) {
                            let s = ssid_val.replace("\"", "");
                            if s == "bjut_wifi" || s == "bjut_sushe" || s == "bjut-wifi" {
                                is_connected = true;
                            }
                        }
                    }

                    if is_connected && !was_connected {
                        #[cfg(desktop)]
                        {
                            use tauri::Emitter;
                            let _ = app_handle.emit("trigger-auto-login", ());
                        }
                        #[cfg(target_os = "android")]
                        {
                            use tauri::Manager;
                            if let Some(window) = app_handle.get_webview_window("main") {
                                let _ = window.eval("if (window.triggerAutoLogin) { window.triggerAutoLogin(); }");
                            }
                        }
                    }
                    was_connected = is_connected;

                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            });

            #[cfg(desktop)]
            {
                #[cfg(not(target_os = "macos"))]
                {
                    use tauri::Manager;
                    if let Some(window) = _app.get_webview_window("main") {
                        let _ = window.set_decorations(false);
                    }
                }
            }
            Ok(())
        })
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet, 
            get_network_info, 
            request_battery_optimizations,
            request_foreground_permissions,
            request_background_permissions,
            start_keep_alive_service,
            stop_keep_alive_service,
            get_local_ip
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
