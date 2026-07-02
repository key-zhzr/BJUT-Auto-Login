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
        let ctx = ndk_context::android_context();
        let vm_ptr = ctx.vm().cast();
        let activity_ptr = ctx.context().cast();

        let vm = unsafe { jni::JavaVM::from_raw(vm_ptr) };
        let _ = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
            let activity = unsafe { jni::objects::JObject::from_raw(env, activity_ptr) };
            
            let class_name = jni::strings::JNIString::from("cn/edu/bjut/al/NetworkHelper");
            if let Ok(class) = env.find_class(&class_name) {
                let method_name = jni::strings::JNIString::from("getNetworkInfo");
                let sig_str = "(Landroid/content/Context;)Ljava/lang/String;";
                let runtime_sig: jni::signature::RuntimeMethodSignature = sig_str.parse().unwrap();
                let sig = jni::signature::MethodSignature::from(&runtime_sig);
                
                if let Ok(jvalue) = env.call_static_method(
                    class,
                    &method_name,
                    &sig,
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
