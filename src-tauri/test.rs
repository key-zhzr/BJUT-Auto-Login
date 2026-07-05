use tauri::Manager;
fn test(app: &tauri::AppHandle) {
    app.run_on_android_context(|env, activity, webview| {});
}
