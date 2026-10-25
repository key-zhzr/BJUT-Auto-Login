#[cfg(target_os = "macos")]
use tauri::menu::ContextMenu;
use tauri::{
    menu::{MenuBuilder, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconEvent},
    Manager,
};

pub(crate) fn show_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        window.show()?;
        if window.is_minimized()? {
            window.unminimize()?;
        }
        window.set_focus()?;
    }
    Ok(())
}

pub(crate) fn click(tray: &tauri::tray::TrayIcon, event: TrayIconEvent) {
    let TrayIconEvent::Click {
        button,
        button_state,
        ..
    } = event
    else {
        return;
    };
    let app = tray.app_handle();
    let activate = button == MouseButton::Left
        && button_state
            == if cfg!(target_os = "macos") {
                MouseButtonState::Down
            } else {
                MouseButtonState::Up
            };
    if activate {
        if let Err(error) = show_window(app) {
            let state = app.state::<std::sync::Arc<crate::AppState>>();
            crate::rust_log(
                app,
                &state,
                "托盘",
                &format!("无法显示主窗口：{error}"),
                "error",
            );
        }
    }
    #[cfg(target_os = "macos")]
    if button == MouseButton::Right && button_state == MouseButtonState::Down {
        let state = app.state::<std::sync::Arc<crate::AppState>>();
        let menu = state.tray_menu.lock().unwrap().clone();
        if let (Some(menu), Some(window)) = (menu, app.get_webview_window("main")) {
            // Do not attach an NSStatusItem menu: AppKit can consume both
            // buttons before the tray view receives its mouse events.
            let _ = menu.popup(window.as_ref().window().clone());
        }
    }
}

pub(crate) fn refresh(app: &tauri::AppHandle, state: &crate::AppState) {
    let Some(tray) = app.tray_by_id("main-tray") else {
        return;
    };
    let network = state.last_network_state.lock().unwrap().clone();
    let label = match network.get("state").and_then(|v| v.as_str()) {
        Some("Online") => "已联网",
        Some("BjutCampus") => "校园网待认证",
        _ => "离线",
    };
    let config = state.config.read().unwrap().clone();
    let settings = if config.tray.validate().is_ok() {
        config.tray.clone()
    } else {
        Default::default()
    };
    let account = config
        .accounts
        .iter()
        .find(|a| a.is_default)
        .or(config.accounts.first())
        .map(|a| a.user.as_str())
        .unwrap_or("未设置");
    let mut tooltip = format!("BJUT-AL · {label}");
    if settings.show_account {
        tooltip.push_str(&format!("\n首选账号：{account}"));
    }
    if settings.show_dual_stack {
        if let Some(report) = state.link_health.lock().unwrap().as_ref() {
            for (name, result) in [("IPv4", &report.ipv4), ("IPv6", &report.ipv6)] {
                tooltip.push_str(&format!(
                    "\n{name}：{}",
                    if result.status == "reachable" {
                        format!("{} ms", result.duration_ms)
                    } else {
                        "未连通".into()
                    }
                ));
            }
        }
    }
    let _ = tray.set_tooltip(Some(tooltip));
    let title = match settings.title.as_str() {
        "status" => label,
        "account" => account,
        _ => "",
    };
    let _ = tray.set_title(Some(title.chars().take(24).collect::<String>()));
    let image = crate::tray_settings::image(&settings).or_else(|| {
        #[cfg(target_os = "macos")]
        {
            tauri::image::Image::from_bytes(include_bytes!("../icons/tray_mac.png")).ok()
        }
        #[cfg(not(target_os = "macos"))]
        {
            app.default_window_icon().cloned()
        }
    });
    let _ = tray.set_icon_with_as_template(image, settings.icon != "custom");
    let Ok(status) =
        MenuItem::with_id(app, "status", format!("状态：{label}"), false, None::<&str>)
    else {
        return;
    };
    let mut menu = MenuBuilder::new(app).text("show", "显示主窗口").separator();
    for item in &settings.menu_order {
        menu = match item.as_str() {
            "status" => menu.item(&status),
            "check" => menu.text("check", "立即检测网络"),
            "login" => menu.text("login", "立即登录"),
            "pause" => menu.text(
                "pause",
                if state
                    .auto_login_paused_until
                    .load(std::sync::atomic::Ordering::SeqCst)
                    > chrono::Utc::now().timestamp()
                {
                    "恢复自动登录"
                } else {
                    "暂停自动登录 1 小时"
                },
            ),
            "accounts" => {
                for (index, account) in config
                    .accounts
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| !a.is_disabled.unwrap_or(false))
                {
                    menu = menu.text(
                        format!("account:{index}"),
                        format!(
                            "{} 首选账号：{}",
                            if account.is_default { "✓" } else { " " },
                            account.user
                        ),
                    );
                }
                menu
            }
            _ => menu,
        };
    }
    let Ok(menu) = menu.separator().text("quit", "退出").build() else {
        return;
    };
    #[cfg(target_os = "macos")]
    {
        *state.tray_menu.lock().unwrap() = Some(menu);
        let _ = tray.set_menu(None::<tauri::menu::Menu<tauri::Wry>>);
        let _ = tray.with_inner_tray_icon(|inner| {
            inner.set_show_menu_on_left_click(false);
            inner.set_show_menu_on_right_click(false);
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = tray.set_menu(Some(menu));
        let _ = tray.set_show_menu_on_left_click(false);
    }
}
