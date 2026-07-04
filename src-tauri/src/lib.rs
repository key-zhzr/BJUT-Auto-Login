// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn get_network_info(_app: tauri::AppHandle) -> serde_json::Value {
    #[cfg(target_os = "android")]
    {
        let mut result = serde_json::json!({});
        let ctx = ndk_context::android_context();
        let vm_ptr = ctx.vm().cast();
        let activity_ptr = ctx.context().cast();

        let vm = unsafe { jni::JavaVM::from_raw(vm_ptr) };
        let _ = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
            let activity = unsafe { jni::objects::JObject::from_raw(env, activity_ptr) };
            
            let class_name = "cn/edu/bjut/al/NetworkHelper";
            if let Ok(class) = env.find_class(class_name) {
                let method_name = "getNetworkInfo";
                let sig = "(Landroid/content/Context;)Ljava/lang/String;";
                
                if let Ok(jvalue) = env.call_static_method(
                    class,
                    method_name,
                    sig,
                    &[jni::objects::JValue::Object(&activity)],
                ) {
                    if let Ok(jobject) = jvalue.l() {
                        let jstring = unsafe { jni::objects::JString::from_raw(env, jobject.as_raw().cast()) };
                        if let Ok(json_str) = jstring.try_to_string(env) {
                            if let Ok(val) = serde_json::from_str(&json_str) {
                                result = val;
                            }
                        }
                    }
                }
            }
            Ok(())
        });
        return result;
    }

    #[cfg(not(target_os = "android"))]
    {
        serde_json::json!({
            "ssid": "bjut-wifi",
            "bssid": "00:11:22:33:44:55",
            "ip": "10.21.221.98"
        })
    }
}

#[tauri::command]
fn request_battery_optimizations(_app: tauri::AppHandle) {
    #[cfg(target_os = "android")]
    {
        let ctx = ndk_context::android_context();
        let vm_ptr = ctx.vm().cast();
        let activity_ptr = ctx.context().cast();

        let vm = unsafe { jni::JavaVM::from_raw(vm_ptr) };
        let _ = vm.attach_current_thread(|mut env| -> jni::errors::Result<()> {
            let activity = unsafe { jni::objects::JObject::from_raw(&mut env, activity_ptr) };
            let class_name = "cn/edu/bjut/al/MainActivity";
            if let Ok(class) = env.find_class(class_name) {
                let method_name = "requestBatteryOptimizations";
                let sig = "()V";
                
                let _ = env.call_method(
                    activity,
                    method_name,
                    sig,
                    &[],
                );
            }
            Ok(())
        });
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            use tauri::Emitter;

            let app_handle = _app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut was_connected = false;
                loop {
                    let mut is_connected = false;

                    #[cfg(target_os = "macos")]
                    {
                        if let Ok(output) = std::process::Command::new("ifconfig").output() {
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            if stdout.contains("inet 10.") {
                                is_connected = true;
                            }
                        }
                    }

                    #[cfg(not(target_os = "macos"))]
                    {
                        let info = get_network_info(app_handle.clone());
                        if let Some(ssid) = info.get("ssid").and_then(|s| s.as_str()) {
                            let s = ssid.replace("\"", "");
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

#[cfg(target_os = "android")]
fn test_android(app: tauri::AppHandle) {
    let env = app.env();
}
