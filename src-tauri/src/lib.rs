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
            // Get SSID/BSSID via netsh wlan (doesn't trigger location prompts)
            let mut cmd = std::process::Command::new("netsh");
            cmd.args(["wlan", "show", "interfaces"]);
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            if let Ok(output) = cmd.output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let trimmed = line.trim();
                    // Match BSSID first (before SSID) to avoid SSID matching BSSID line
                    if trimmed.starts_with("BSSID") {
                        if let Some(idx) = trimmed.find(':') {
                            bssid = trimmed[idx + 1..].trim().to_string();
                        }
                    } else if trimmed.starts_with("SSID") && !trimmed.starts_with("SSID ") {
                        // Skip lines like "SSID 1" in profile listings
                        if let Some(idx) = trimmed.find(':') {
                            ssid = trimmed[idx + 1..].trim().to_string();
                        }
                    }
                }
            }

            // Get IP via PowerShell physical adapter command (consistent with Wi-Fi check)
            let mut ip_cmd = std::process::Command::new("powershell");
            ip_cmd.args([
                "-NoProfile",
                "-Command",
                "(Get-NetAdapter -Physical | Where-Object Status -eq 'Up' | Get-NetIPAddress -AddressFamily IPv4).IPAddress"
            ]);
            ip_cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            if let Ok(output) = ip_cmd.output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let trimmed = stdout.trim();
                if !trimmed.is_empty() {
                    if let Some(first_line) = trimmed.lines().next() {
                        ip = first_line.trim().to_string();
                    }
                }
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
        let mut cmd = std::process::Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-Command",
            "(Get-NetAdapter -Physical | Where-Object Status -eq 'Up' | Get-NetIPAddress -AddressFamily IPv4).IPAddress"
        ]);
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        if let Ok(output) = cmd.output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let trimmed = stdout.trim();
            if !trimmed.is_empty() {
                if let Some(first_line) = trimmed.lines().next() {
                    ip = first_line.trim().to_string();
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(target_os = "android")]
        {
            if let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() {
                if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) } {
                    if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                        if let Ok(class) = env.find_class("cn/edu/bjut/al/NetworkHelper") {
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .setup(|_app| {
            // Background network poller.
            // On desktop, it emits to frontend. On Android, it evaluates JS directly.
            let app_handle = _app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut last_ip = String::new();
                loop {
                    let current_ip = get_local_ip();
                    if !current_ip.is_empty() {
                        if !last_ip.is_empty() && current_ip != last_ip {
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
                        last_ip = current_ip;
                    }

                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
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
                    window.on_window_event(move |event| {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                            api.prevent_close();
                            let _ = window_clone.hide();
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
        .plugin(tauri_plugin_http::init())
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
            set_dock_visible
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
