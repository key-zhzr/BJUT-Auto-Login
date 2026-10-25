use std::sync::Mutex;
#[cfg(desktop)]
use tauri::Emitter;
static PENDING: Mutex<Option<String>> = Mutex::new(None);
#[cfg(any(desktop, test))]
fn parse(value: &str) -> Option<&str> {
    let value = value.strip_prefix("bjut-al://")?;
    let action = value.strip_suffix('/').unwrap_or(value);
    match action {
        "dashboard" | "accounts" | "billing" | "check" | "login" => Some(action),
        _ => None,
    }
}
#[tauri::command]
pub(crate) fn take_quick_action() -> Option<String> {
    PENDING.lock().unwrap().take()
}
#[cfg(desktop)]
pub(crate) fn install(app: &tauri::AppHandle) {
    use tauri_plugin_deep_link::DeepLinkExt;
    let links = app.deep_link();
    if let Ok(Some(urls)) = links.get_current() {
        *PENDING.lock().unwrap() = urls
            .iter()
            .find_map(|url| parse(url.as_str()).map(str::to_owned));
    }
    let handle = app.clone();
    links.on_open_url(move |event| {
        if let Some(action) = event
            .urls()
            .iter()
            .find_map(|url| parse(url.as_str()).map(str::to_owned))
        {
            *PENDING.lock().unwrap() = Some(action.clone());
            let window_app = handle.clone();
            let _ = handle.run_on_main_thread(move || {
                let _ = crate::desktop_tray::show_window(&window_app);
            });
            let _ = handle.emit("quick-action", action);
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_cannot_carry_credentials_or_commands() {
        assert_eq!(parse("bjut-al://billing"), Some("billing"));
        for bad in [
            "https://login",
            "bjut-al://login?password=x",
            "bjut-al://root@login",
            "bjut-al://login/#x",
            "bjut-al://check;reboot",
        ] {
            assert!(parse(bad).is_none());
        }
    }
}
