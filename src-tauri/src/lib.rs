// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn get_network_info(_app: tauri::AppHandle) -> serde_json::Value {
    #[cfg(target_os = "android")]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        tauri::wry::prelude::dispatch(move |env, activity, _webview| {
            let mut result = serde_json::json!({});
            if let Ok(class) = tauri::wry::prelude::find_class(env, activity, "cn.edu.bjut.al.NetworkHelper".into()) {
                if let Ok(jvalue) = env.call_static_method(
                    class,
                    "getNetworkInfo",
                    "(Landroid/content/Context;)Ljava/lang/String;",
                    &[jni::objects::JValue::Object(activity)],
                ) {
                    if let Ok(jobject) = jvalue.l() {
                        let jstring: jni::objects::JString = jobject.into();
                        if let Ok(rust_str) = env.get_string(&jstring).map(|s| { let s: String = s.into(); s }) {
                            if let Ok(val) = serde_json::from_str(&rust_str) {
                                result = val;
                            }
                        }
                    }
                }
            }
            let _ = tx.send(result);
        });
        return rx.recv_timeout(std::time::Duration::from_secs(5))
            .unwrap_or(serde_json::json!({"ssid": "", "bssid": "", "ip": ""}));
    }

    #[cfg(not(target_os = "android"))]
    {
        let mut ssid = String::new();
        let mut bssid = String::new();
        let mut ip = String::new();

        #[cfg(target_os = "macos")]
        {
            use tauri::Manager;
            static PROMPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !PROMPTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
                let _ = _app.run_on_main_thread(|| {
                    unsafe {
                        let manager = objc2_core_location::CLLocationManager::new();
                        manager.requestWhenInUseAuthorization();
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
            if let Ok(output) = std::process::Command::new("netsh").args(["wlan", "show", "interfaces"]).output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let line = line.trim();
                    if line.starts_with("SSID") && !line.starts_with("BSSID") {
                        if let Some(idx) = line.find(':') {
                            ssid = line[idx + 1..].trim().to_string();
                        }
                    }
                    if line.starts_with("BSSID") {
                        if let Some(idx) = line.find(':') {
                            bssid = line[idx + 1..].trim().to_string();
                        }
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
        tauri::wry::prelude::dispatch(move |env, activity, _webview| {
            let _ = env.call_method(
                activity,
                "requestBatteryOptimizations",
                "()V",
                &[],
            );
        });
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            use tauri::Emitter;

            // Background network poller - desktop only.
            // On Android, the frontend's own startNetworkCheckLoop handles auto-login.
            #[cfg(desktop)]
            {
                let app_handle = _app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let mut was_connected = false;
                    loop {
                        let mut is_connected = false;

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
                            let _ = app_handle.emit("trigger-auto-login", ());
                        }
                        was_connected = is_connected;

                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                });
            }

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
        .invoke_handler(tauri::generate_handler![greet, get_network_info, request_battery_optimizations])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}


