// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn get_network_info(app: tauri::AppHandle) -> serde_json::Value {
    #[cfg(target_os = "android")]
    {
        let mut result = serde_json::json!({});
        if let Some(ctx) = ndk_context::android_context().ext() {
            // Unsafe block to get JNI Env
            if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) } {
                if let Ok(mut env) = vm.attach_current_thread() {
                    let activity = ctx.context(); // This is a jobject pointing to Context
                    
                    if let Ok(class) = env.find_class("cn/edu/bjut/al/NetworkHelper") {
                        if let Ok(jvalue) = env.call_static_method(
                            class,
                            "getNetworkInfo",
                            "(Landroid/content/Context;)Ljava/lang/String;",
                            &[jni::objects::JValue::Object(unsafe { jni::objects::JObject::from_raw(activity.cast()) }.as_ref())],
                        ) {
                            if let Ok(jstring) = jvalue.l() {
                                if let Ok(s) = env.get_string((&jstring).into()) {
                                    let json_str: String = s.into();
                                    if let Ok(val) = serde_json::from_str(&json_str) {
                                        result = val;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet, get_network_info])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
