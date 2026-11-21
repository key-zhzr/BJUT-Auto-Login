#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq, Eq)]
pub struct WindowsBackdropStatus {
    pub supported: bool,
    pub active: bool,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn resolve_opaque_background_rgba(theme: &str, color_scheme: &str) -> (u8, u8, u8, u8) {
    let is_dark = color_scheme.eq_ignore_ascii_case("dark");
    match (theme.trim().to_ascii_lowercase().as_str(), is_dark) {
        ("winui", true) => (32, 32, 32, 255),
        ("winui", false) => (243, 243, 243, 255),
        (_, true) => (15, 23, 42, 255),
        (_, false) => (238, 242, 247, 255),
    }
}

#[cfg(target_os = "windows")]
fn is_windows_11_mica_supported() -> bool {
    #[repr(C)]
    struct OsVersionInfoExW {
        dw_os_version_info_size: u32,
        dw_major_version: u32,
        dw_minor_version: u32,
        dw_build_number: u32,
        dw_platform_id: u32,
        sz_csd_version: [u16; 128],
        w_service_pack_major: u16,
        w_service_pack_minor: u16,
        w_suite_mask: u16,
        w_product_type: u8,
        w_reserved: u8,
    }

    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(lp_version_information: *mut OsVersionInfoExW) -> i32;
    }

    let mut info: OsVersionInfoExW = unsafe { std::mem::zeroed() };
    info.dw_os_version_info_size = std::mem::size_of::<OsVersionInfoExW>() as u32;
    let status = unsafe { RtlGetVersion(&mut info) };
    status == 0
        && (info.dw_major_version > 10
            || (info.dw_major_version == 10 && info.dw_build_number >= 22000))
}

#[cfg(target_os = "windows")]
pub fn apply_windows_backdrop_to_window(
    window: &tauri::WebviewWindow,
    theme: &str,
    color_scheme: &str,
) -> WindowsBackdropStatus {
    let is_dark = color_scheme.eq_ignore_ascii_case("dark");
    let _ = window.set_theme(Some(if is_dark {
        tauri::Theme::Dark
    } else {
        tauri::Theme::Light
    }));

    let supported = is_windows_11_mica_supported();
    if theme.trim().eq_ignore_ascii_case("winui") && supported {
        let _ = window.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
        let effect = if is_dark {
            tauri::window::Effect::MicaDark
        } else {
            tauri::window::Effect::MicaLight
        };
        let _ = window.set_effects(tauri::window::EffectsBuilder::new().effect(effect).build());
        return WindowsBackdropStatus {
            supported: true,
            active: true,
        };
    }

    let _ = window.set_effects(None);
    let (r, g, b, a) = resolve_opaque_background_rgba(theme, color_scheme);
    let _ = window.set_background_color(Some(tauri::window::Color(r, g, b, a)));
    WindowsBackdropStatus {
        supported,
        active: false,
    }
}

#[tauri::command]
pub fn sync_windows_backdrop(
    app: tauri::AppHandle,
    theme: String,
    color_scheme: String,
) -> Result<WindowsBackdropStatus, String> {
    #[cfg(target_os = "windows")]
    {
        use tauri::Manager;
        if let Some(window) = app.get_webview_window("main") {
            return Ok(apply_windows_backdrop_to_window(
                &window,
                &theme,
                &color_scheme,
            ));
        }
        return Ok(WindowsBackdropStatus {
            supported: is_windows_11_mica_supported(),
            active: false,
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, theme, color_scheme);
        Ok(WindowsBackdropStatus {
            supported: false,
            active: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_opaque_background_rgba;

    #[test]
    fn resolve_opaque_background_rgba_matches_theme_fallbacks() {
        assert_eq!(
            resolve_opaque_background_rgba("winui", "dark"),
            (32, 32, 32, 255)
        );
        assert_eq!(
            resolve_opaque_background_rgba("winui", "light"),
            (243, 243, 243, 255)
        );
        assert_eq!(
            resolve_opaque_background_rgba("basic", "dark"),
            (15, 23, 42, 255)
        );
        assert_eq!(
            resolve_opaque_background_rgba("basic", "light"),
            (238, 242, 247, 255)
        );
    }
}
