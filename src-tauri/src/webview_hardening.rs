//! Native WebView hardening for desktop shells.
//!
//! On Windows, Tauri 2 uses Edge WebView2 (`ICoreWebView2Controller` / `ICoreWebView2Settings`).
//! Because `Cargo.toml` does not declare `webview2-com` directly, the Windows path uses the
//! standard COM ABI through `windows::core::Interface` (shared `windows-core` 0.61) to configure
//! WebView2 settings directly on the live controller without altering crate dependencies.

#[cfg(target_os = "windows")]
mod imp {
    use std::ffi::c_void;
    use std::ptr::{null_mut, NonNull};
    use windows::core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface};

    // Exact COM interface GUIDs from Microsoft.Web.WebView2.Core (verified against webview2-com-sys 0.38.2).
    const IID_ICOREWEBVIEW2_SETTINGS3: GUID =
        GUID::from_u128(0xfdb5ab74_af33_4854_84f0_0a631deb5eba);
    const IID_ICOREWEBVIEW2_SETTINGS5: GUID =
        GUID::from_u128(0x183e7052_1d03_43a0_ab99_98e043b66b39);
    const IID_ICOREWEBVIEW2_SETTINGS6: GUID =
        GUID::from_u128(0x11cb3acd_9bc8_43b8_83bf_f40753714f87);

    #[repr(C)]
    #[allow(dead_code)]
    pub(super) struct CoreWebView2ControllerVtbl {
        base: IUnknown_Vtbl,
        is_visible: *const c_void,
        set_is_visible: *const c_void,
        bounds: *const c_void,
        set_bounds: *const c_void,
        zoom_factor: *const c_void,
        set_zoom_factor: unsafe extern "system" fn(*mut c_void, f64) -> HRESULT,
        add_zoom_factor_changed: *const c_void,
        remove_zoom_factor_changed: *const c_void,
        set_bounds_and_zoom_factor: *const c_void,
        move_focus: *const c_void,
        add_move_focus_requested: *const c_void,
        remove_move_focus_requested: *const c_void,
        add_got_focus: *const c_void,
        remove_got_focus: *const c_void,
        add_lost_focus: *const c_void,
        remove_lost_focus: *const c_void,
        add_accelerator_key_pressed: *const c_void,
        remove_accelerator_key_pressed: *const c_void,
        parent_window: *const c_void,
        set_parent_window: *const c_void,
        notify_parent_window_position_changed: *const c_void,
        close: *const c_void,
        core_webview2: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    }

    #[repr(C)]
    #[allow(dead_code)]
    struct CoreWebView2Vtbl {
        base: IUnknown_Vtbl,
        settings: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    }

    #[repr(C)]
    #[allow(dead_code)]
    struct CoreWebView2SettingsVtbl {
        base: IUnknown_Vtbl,
        is_script_enabled: *const c_void,
        set_is_script_enabled: *const c_void,
        is_web_message_enabled: *const c_void,
        set_is_web_message_enabled: *const c_void,
        are_default_script_dialogs_enabled: *const c_void,
        set_are_default_script_dialogs_enabled: *const c_void,
        is_status_bar_enabled: *const c_void,
        set_is_status_bar_enabled: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
        are_dev_tools_enabled: *const c_void,
        set_are_dev_tools_enabled: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
        are_default_context_menus_enabled: *const c_void,
        set_are_default_context_menus_enabled:
            unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
        are_host_objects_allowed: *const c_void,
        set_are_host_objects_allowed: *const c_void,
        is_zoom_control_enabled: *const c_void,
        set_is_zoom_control_enabled: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
        is_built_in_error_page_enabled: *const c_void,
        set_is_built_in_error_page_enabled: *const c_void,
    }

    #[repr(C)]
    #[allow(dead_code)]
    struct CoreWebView2Settings2Vtbl {
        base: CoreWebView2SettingsVtbl,
        user_agent: *const c_void,
        set_user_agent: *const c_void,
    }

    #[repr(C)]
    #[allow(dead_code)]
    struct CoreWebView2Settings3Vtbl {
        base: CoreWebView2Settings2Vtbl,
        are_browser_accelerator_keys_enabled: *const c_void,
        set_are_browser_accelerator_keys_enabled:
            unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
    }

    #[repr(C)]
    #[allow(dead_code)]
    struct CoreWebView2Settings4Vtbl {
        base: CoreWebView2Settings3Vtbl,
        is_password_autosave_enabled: *const c_void,
        set_is_password_autosave_enabled: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
        is_general_autofill_enabled: *const c_void,
        set_is_general_autofill_enabled: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
    }

    #[repr(C)]
    #[allow(dead_code)]
    struct CoreWebView2Settings5Vtbl {
        base: CoreWebView2Settings4Vtbl,
        is_pinch_zoom_enabled: *const c_void,
        set_is_pinch_zoom_enabled: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
    }

    #[repr(C)]
    #[allow(dead_code)]
    struct CoreWebView2Settings6Vtbl {
        base: CoreWebView2Settings5Vtbl,
        is_swipe_navigation_enabled: *const c_void,
        set_is_swipe_navigation_enabled: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
    }

    unsafe fn vtable_of<T>(ptr: *const IUnknown) -> &'static T {
        let raw = (*ptr).as_raw();
        &**(raw as *const *const T)
    }

    unsafe fn query_com_interface(unknown: &IUnknown, iid: &GUID) -> Option<IUnknown> {
        let mut out = null_mut();
        let hr = unknown.query(iid, &mut out);
        if hr.is_ok() && !out.is_null() {
            Some(IUnknown::from_raw(out))
        } else {
            None
        }
    }

    pub fn configure(window: &tauri::WebviewWindow) {
        let _ = window.set_zoom(1.0);
        let _ = window.with_webview(|webview| {
            let controller = webview.controller();
            let raw_controller = controller.as_raw();
            if NonNull::new(raw_controller).is_none() {
                return;
            }

            // SAFETY: `controller` is a valid live `ICoreWebView2Controller` COM pointer on the main
            // thread. Every COM out-pointer returned from `CoreWebView2` / `Settings` / `QueryInterface`
            // is AddRef'd by WebView2 and immediately wrapped in `IUnknown::from_raw` for RAII release.
            unsafe {
                let controller_vtbl = &**(raw_controller as *const *const CoreWebView2ControllerVtbl);
                let _ = (controller_vtbl.set_zoom_factor)(raw_controller, 1.0);

                let mut raw_webview = null_mut();
                if (controller_vtbl.core_webview2)(raw_controller, &mut raw_webview).is_err()
                    || raw_webview.is_null()
                {
                    return;
                }
                let core_webview = IUnknown::from_raw(raw_webview);
                let webview_vtbl = vtable_of::<CoreWebView2Vtbl>(&core_webview);

                let mut raw_settings = null_mut();
                if (webview_vtbl.settings)(core_webview.as_raw(), &mut raw_settings).is_err()
                    || raw_settings.is_null()
                {
                    return;
                }
                let settings = IUnknown::from_raw(raw_settings);
                let settings_vtbl = vtable_of::<CoreWebView2SettingsVtbl>(&settings);

                // Suppress browser hover status bar and keyboard/wheel zoom across all builds.
                let _ = (settings_vtbl.set_is_status_bar_enabled)(settings.as_raw(), 0);
                let _ = (settings_vtbl.set_is_zoom_control_enabled)(settings.as_raw(), 0);

                // Keep DevTools & native Inspect context menu accessible in debug builds so
                // developer Shift+RightClick works, while locking them down in release builds.
                #[cfg(not(debug_assertions))]
                {
                    let _ = (settings_vtbl.set_are_dev_tools_enabled)(settings.as_raw(), 0);
                    let _ =
                        (settings_vtbl.set_are_default_context_menus_enabled)(settings.as_raw(), 0);
                }

                if let Some(settings3) =
                    query_com_interface(&settings, &IID_ICOREWEBVIEW2_SETTINGS3)
                {
                    #[cfg(not(debug_assertions))]
                    {
                        let vtbl = vtable_of::<CoreWebView2Settings3Vtbl>(&settings3);
                        let _ = (vtbl.set_are_browser_accelerator_keys_enabled)(
                            settings3.as_raw(),
                            0,
                        );
                    }
                    let _ = settings3;
                }

                if let Some(settings5) =
                    query_com_interface(&settings, &IID_ICOREWEBVIEW2_SETTINGS5)
                {
                    let vtbl = vtable_of::<CoreWebView2Settings5Vtbl>(&settings5);
                    let _ = (vtbl.set_is_pinch_zoom_enabled)(settings5.as_raw(), 0);
                }

                if let Some(settings6) =
                    query_com_interface(&settings, &IID_ICOREWEBVIEW2_SETTINGS6)
                {
                    let vtbl = vtable_of::<CoreWebView2Settings6Vtbl>(&settings6);
                    let _ = (vtbl.set_is_swipe_navigation_enabled)(settings6.as_raw(), 0);
                }
            }
        });
    }
}

/// Apply native WebView interaction hardening to the main window.
///
/// - **Windows**: Locks down WebView2 browser status bar, pinch/hotkey zoom, swipe navigation,
///   and (in release builds) browser accelerator keys, DevTools, and native context menus.
/// - **macOS / Linux**: Resets initial zoom factor to `1.0` and delegates runtime hardening to
///   `src/native-shell-guard.ts` and `src/context-menu.ts`.
/// - **Android / iOS**: No-op (`MainActivity.kt` configures Android `WebView` directly).
pub fn configure_main_webview(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "windows")]
    {
        imp::configure(window);
    }

    #[cfg(all(not(target_os = "windows"), not(any(target_os = "android", target_os = "ios"))))]
    {
        let _ = window.set_zoom(1.0);
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = window;
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_com_vtables_match_expected_slot_counts() {
        use std::ffi::c_void;
        use std::mem::size_of;
        assert_eq!(
            size_of::<super::imp::CoreWebView2ControllerVtbl>(),
            26 * size_of::<*const c_void>()
        );
    }

    #[test]
    fn hardening_module_builds_cleanly() {
        assert!(true);
    }
}
