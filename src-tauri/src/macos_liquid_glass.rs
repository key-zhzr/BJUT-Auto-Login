use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MacosLiquidGlassCapabilities {
    pub supported: bool,
    pub engine: &'static str,
    pub interactive_supported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiquidGlassSlot {
    pub id: String,
    pub placement: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub corner_radius: f64,
    pub style: String,
    pub visible: bool,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub tint_rgba: Option<[f64; 4]>,
}

#[derive(Debug, Clone, PartialEq)]
struct NormalizedSlot {
    id: String,
    is_overlay: bool,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    corner_radius: f64,
    is_clear: bool,
    visible: bool,
    interactive: bool,
    tint_rgba: Option<[f64; 4]>,
}

fn sanitize_slot(raw: LiquidGlassSlot) -> Option<NormalizedSlot> {
    let id = raw.id.trim().to_string();
    if id.is_empty() {
        return None;
    }

    let valid_dimensions =
        raw.width.is_finite() && raw.height.is_finite() && raw.width > 0.0 && raw.height > 0.0;
    let visible = raw.visible && valid_dimensions && raw.x.is_finite() && raw.y.is_finite();

    let max_radius = if valid_dimensions {
        raw.width.min(raw.height) * 0.5
    } else {
        0.0
    };
    let corner_radius = if raw.corner_radius.is_finite() {
        raw.corner_radius.clamp(0.0, max_radius)
    } else {
        0.0
    };

    let tint_rgba = raw.tint_rgba.and_then(|rgba| {
        if rgba.iter().all(|c| c.is_finite()) {
            Some([
                rgba[0].clamp(0.0, 1.0),
                rgba[1].clamp(0.0, 1.0),
                rgba[2].clamp(0.0, 1.0),
                rgba[3].clamp(0.0, 1.0),
            ])
        } else {
            None
        }
    });

    Some(NormalizedSlot {
        id,
        is_overlay: raw.placement == "overlay",
        x: if raw.x.is_finite() { raw.x } else { 0.0 },
        y: if raw.y.is_finite() { raw.y } else { 0.0 },
        width: if raw.width.is_finite() {
            raw.width.max(0.0)
        } else {
            0.0
        },
        height: if raw.height.is_finite() {
            raw.height.max(0.0)
        } else {
            0.0
        },
        corner_radius,
        is_clear: raw.style == "clear",
        visible,
        interactive: raw.interactive,
        tint_rgba,
    })
}

fn capabilities_impl() -> MacosLiquidGlassCapabilities {
    #[cfg(target_os = "macos")]
    {
        imp::capabilities()
    }
    #[cfg(not(target_os = "macos"))]
    {
        MacosLiquidGlassCapabilities {
            supported: false,
            engine: "none",
            interactive_supported: false,
        }
    }
}

#[tauri::command]
pub fn get_macos_liquid_glass_capabilities() -> MacosLiquidGlassCapabilities {
    capabilities_impl()
}

#[tauri::command]
pub fn sync_macos_liquid_glass(
    window: tauri::WebviewWindow,
    slots: Vec<LiquidGlassSlot>,
) -> Result<(), String> {
    let normalized: Vec<NormalizedSlot> = slots.into_iter().filter_map(sanitize_slot).collect();
    #[cfg(target_os = "macos")]
    {
        let window_for_main = window.clone();
        window
            .run_on_main_thread(move || {
                imp::apply_slots(&window_for_main, normalized);
            })
            .map_err(|err| format!("failed to sync macOS glass slots on main thread: {err}"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, normalized);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{MacosLiquidGlassCapabilities, NormalizedSlot};
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSAnimationContext, NSAutoresizingMaskOptions, NSColor, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
        NSWindowOrderingMode,
    };
    use objc2_core_foundation::{CGFloat, CGPoint, CGSize};
    use objc2_foundation::{ns_string, NSNumber, NSObjectNSKeyValueCoding, NSPoint, NSRect};
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};

    define_class!(
        #[unsafe(super(NSView))]
        #[thread_kind = MainThreadOnly]
        #[name = "BjutGlassUnderlayHostView"]
        struct BjutGlassUnderlayHostView;

        impl BjutGlassUnderlayHostView {
            #[unsafe(method(isFlipped))]
            fn is_flipped(&self) -> bool {
                true
            }
        }
    );

    impl BjutGlassUnderlayHostView {
        fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
            let this = mtm.alloc::<Self>().set_ivars(());
            unsafe { msg_send![super(this), initWithFrame: frame] }
        }
    }

    define_class!(
        #[unsafe(super(NSView))]
        #[thread_kind = MainThreadOnly]
        #[name = "BjutGlassOverlayHostView"]
        struct BjutGlassOverlayHostView;

        impl BjutGlassOverlayHostView {
            #[unsafe(method(isFlipped))]
            fn is_flipped(&self) -> bool {
                true
            }

            #[unsafe(method(hitTest:))]
            fn hit_test(&self, _point: NSPoint) -> *mut NSView {
                std::ptr::null_mut()
            }
        }
    );

    impl BjutGlassOverlayHostView {
        fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
            let this = mtm.alloc::<Self>().set_ivars(());
            unsafe { msg_send![super(this), initWithFrame: frame] }
        }
    }

    enum SlotView {
        Glass(Retained<NSView>),
        VisualEffect(Retained<NSVisualEffectView>),
    }

    impl SlotView {
        fn as_view(&self) -> &NSView {
            match self {
                Self::Glass(v) => v,
                Self::VisualEffect(v) => v,
            }
        }
    }

    struct NativeGlassHost {
        underlay_host: Retained<BjutGlassUnderlayHostView>,
        overlay_host: Retained<BjutGlassOverlayHostView>,
        overlay_content: Retained<BjutGlassOverlayHostView>,
        container_view: Option<Retained<NSView>>,
        slots: HashMap<String, SlotView>,
        underlay_transparent: bool,
    }

    thread_local! {
        static HOSTS: RefCell<HashMap<usize, NativeGlassHost>> = RefCell::new(HashMap::new());
    }

    pub(super) fn capabilities() -> MacosLiquidGlassCapabilities {
        if let Some(glass_cls) = AnyClass::get(c"NSGlassEffectView") {
            let interactive_supported = glass_cls.responds_to(sel!(setEffectIsInteractive:));
            MacosLiquidGlassCapabilities {
                supported: true,
                engine: "ns-glass-effect",
                interactive_supported,
            }
        } else {
            MacosLiquidGlassCapabilities {
                supported: true,
                engine: "ns-visual-effect",
                interactive_supported: false,
            }
        }
    }

    fn find_wkwebview(content_view: &NSView) -> Option<Retained<NSView>> {
        content_view.subviews().iter().find(|view| {
            view.class()
                .name()
                .to_str()
                .is_ok_and(|name| name.contains("WebView"))
        })
    }

    fn create_ns_glass_effect_view(glass_cls: &AnyClass) -> Retained<NSView> {
        let allocated: Allocated<NSView> = unsafe { msg_send![glass_cls, alloc] };
        unsafe { msg_send![allocated, initWithFrame: NSRect::ZERO] }
    }

    fn create_ns_glass_container_view(container_cls: &AnyClass, frame: NSRect) -> Retained<NSView> {
        let allocated: Allocated<NSView> = unsafe { msg_send![container_cls, alloc] };
        let container: Retained<NSView> = unsafe { msg_send![allocated, initWithFrame: frame] };
        let spacing: CGFloat = 18.0;
        let _: () = unsafe { msg_send![&*container, setSpacing: spacing] };
        container
    }

    fn create_visual_effect_view(mtm: MainThreadMarker) -> Retained<NSVisualEffectView> {
        let view = NSVisualEffectView::initWithFrame(mtm.alloc(), NSRect::ZERO);
        view.setMaterial(NSVisualEffectMaterial::Sidebar);
        view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        view.setState(NSVisualEffectState::FollowsWindowActiveState);
        view
    }

    fn update_glass_properties(view: &NSView, slot: &NormalizedSlot, glass_cls: &AnyClass) {
        let style: isize = if slot.is_clear { 1 } else { 0 };
        let radius: CGFloat = slot.corner_radius;
        let tint: Option<Retained<NSColor>> = slot
            .tint_rgba
            .map(|[r, g, b, a]| NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, a));
        unsafe {
            let _: () = msg_send![view, setStyle: style];
            let _: () = msg_send![view, setCornerRadius: radius];
            let tint_ptr: Option<&NSColor> = tint.as_deref();
            let _: () = msg_send![view, setTintColor: tint_ptr];
            if glass_cls.responds_to(sel!(setEffectIsInteractive:)) {
                let _: () = msg_send![view, setEffectIsInteractive: slot.interactive];
            }
        }
    }

    fn apply_webview_transparency(
        ns_window: &NSWindow,
        webview: Option<&NSView>,
        transparent: bool,
    ) {
        ns_window.setOpaque(!transparent);
        if transparent {
            let clear = NSColor::clearColor();
            ns_window.setBackgroundColor(Some(&clear));
        } else {
            ns_window.setBackgroundColor(None);
        }
        if let Some(wk) = webview {
            let flag = NSNumber::numberWithBool(!transparent);
            unsafe {
                wk.setValue_forKey(Some(&*flag), ns_string!("drawsBackground"));
            }
        }
    }

    pub(super) fn apply_slots(window: &tauri::WebviewWindow, slots: Vec<NormalizedSlot>) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Ok(ns_window_ptr) = window.ns_window() else {
            return;
        };
        if ns_window_ptr.is_null() {
            return;
        }
        let window_key = ns_window_ptr as usize;
        // SAFETY: Dispatched to main thread and checked for non-null AppKit NSWindow pointer.
        let ns_window = unsafe { &*(ns_window_ptr as *const NSWindow) };
        let Some(content_view) = ns_window.contentView() else {
            return;
        };
        let webview = find_wkwebview(&content_view);
        let glass_cls = AnyClass::get(c"NSGlassEffectView");

        HOSTS.with(|cell| {
            let mut hosts = cell.borrow_mut();
            let host = hosts.entry(window_key).or_insert_with(|| {
                let bounds = content_view.bounds();
                let resize_mask = NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable;

                let underlay_host = BjutGlassUnderlayHostView::new(mtm, bounds);
                underlay_host.setAutoresizingMask(resize_mask);
                content_view.addSubview_positioned_relativeTo(
                    &underlay_host,
                    NSWindowOrderingMode::Below,
                    webview.as_deref(),
                );

                let overlay_host = BjutGlassOverlayHostView::new(mtm, bounds);
                overlay_host.setAutoresizingMask(resize_mask);
                content_view.addSubview_positioned_relativeTo(
                    &overlay_host,
                    NSWindowOrderingMode::Above,
                    webview.as_deref(),
                );

                let overlay_content = BjutGlassOverlayHostView::new(mtm, bounds);
                overlay_content.setAutoresizingMask(resize_mask);

                let container_view = AnyClass::get(c"NSGlassEffectContainerView").map(|cls| {
                    let container = create_ns_glass_container_view(cls, bounds);
                    container.setAutoresizingMask(resize_mask);
                    unsafe {
                        let _: () = msg_send![&*container, setContentView: &*overlay_content];
                    }
                    overlay_host.addSubview(&container);
                    container
                });

                if container_view.is_none() {
                    overlay_host.addSubview(&overlay_content);
                }

                NativeGlassHost {
                    underlay_host,
                    overlay_host,
                    overlay_content,
                    container_view,
                    slots: HashMap::new(),
                    underlay_transparent: false,
                }
            });

            // Keep host bounds synced in case WKWebView resized without NSAutoresizing firing yet.
            let bounds = content_view.bounds();
            host.underlay_host.setFrame(bounds);
            host.overlay_host.setFrame(bounds);
            if let Some(container) = &host.container_view {
                container.setFrame(bounds);
            }
            host.overlay_content.setFrame(bounds);

            NSAnimationContext::beginGrouping();
            let ctx = NSAnimationContext::currentContext();
            ctx.setDuration(0.0);
            ctx.setAllowsImplicitAnimation(false);

            let mut seen_ids = HashSet::with_capacity(slots.len());
            let mut any_underlay_visible = false;

            for slot in &slots {
                seen_ids.insert(slot.id.clone());

                if slot.is_overlay && glass_cls.is_none() {
                    if let Some(existing) = host.slots.get(&slot.id) {
                        existing.as_view().setHidden(true);
                    }
                    continue;
                }

                if !slot.is_overlay && slot.visible {
                    any_underlay_visible = true;
                }

                let existing = host.slots.get(&slot.id);
                let needs_recreate = match existing {
                    Some(SlotView::Glass(_)) => glass_cls.is_none(),
                    Some(SlotView::VisualEffect(_)) => glass_cls.is_some() || slot.is_overlay,
                    None => false,
                };
                if needs_recreate {
                    if let Some(old) = host.slots.remove(&slot.id) {
                        old.as_view().removeFromSuperview();
                    }
                }

                let entry = host.slots.entry(slot.id.clone()).or_insert_with(|| {
                    if let Some(cls) = glass_cls {
                        let view = create_ns_glass_effect_view(cls);
                        if slot.is_overlay {
                            host.overlay_content.addSubview(&view);
                        } else {
                            host.underlay_host.addSubview(&view);
                        }
                        SlotView::Glass(view)
                    } else {
                        let view = create_visual_effect_view(mtm);
                        host.underlay_host.addSubview(&view);
                        SlotView::VisualEffect(view)
                    }
                });

                if !slot.visible {
                    entry.as_view().setHidden(true);
                    continue;
                }

                let rect = NSRect::new(
                    CGPoint::new(slot.x, slot.y),
                    CGSize::new(slot.width, slot.height),
                );
                entry.as_view().setFrame(rect);
                match &*entry {
                    SlotView::Glass(glass_view) => {
                        if let Some(cls) = glass_cls {
                            update_glass_properties(glass_view, slot, cls);
                        }
                    }
                    SlotView::VisualEffect(effect_view) => {
                        if slot.corner_radius > 0.0 {
                            effect_view.setWantsLayer(true);
                            let layer: Option<Retained<AnyObject>> =
                                unsafe { msg_send![&**effect_view, layer] };
                            if let Some(layer) = layer {
                                unsafe {
                                    let _: () =
                                        msg_send![&*layer, setCornerRadius: slot.corner_radius];
                                    let _: () = msg_send![&*layer, setMasksToBounds: true];
                                }
                            }
                        }
                    }
                }
                entry.as_view().setHidden(false);
            }

            for (id, slot_view) in &host.slots {
                if !seen_ids.contains(id) {
                    slot_view.as_view().setHidden(true);
                }
            }

            if host.underlay_transparent != any_underlay_visible {
                host.underlay_transparent = any_underlay_visible;
                apply_webview_transparency(ns_window, webview.as_deref(), any_underlay_visible);
            }

            NSAnimationContext::endGrouping();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_report_known_engine() {
        let caps = capabilities_impl();
        if cfg!(target_os = "macos") {
            assert!(caps.supported);
            assert!(caps.engine == "ns-glass-effect" || caps.engine == "ns-visual-effect");
        } else {
            assert!(!caps.supported);
            assert_eq!(caps.engine, "none");
            assert!(!caps.interactive_supported);
        }
    }

    #[test]
    fn sanitize_slot_clamps_invalid_dimensions_and_tints() {
        assert!(sanitize_slot(LiquidGlassSlot {
            id: "   ".into(),
            placement: "overlay".into(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 60.0,
            corner_radius: 30.0,
            style: "clear".into(),
            visible: true,
            interactive: true,
            tint_rgba: None,
        })
        .is_none());

        let sanitized = sanitize_slot(LiquidGlassSlot {
            id: "dock-droplet".into(),
            placement: "overlay".into(),
            x: 10.0,
            y: f64::NAN,
            width: 120.0,
            height: 60.0,
            corner_radius: 99.0,
            style: "clear".into(),
            visible: true,
            interactive: true,
            tint_rgba: Some([1.5, -0.2, 0.5, 0.8]),
        })
        .unwrap();
        assert!(!sanitized.visible);
        assert_eq!(sanitized.corner_radius, 30.0);
        assert_eq!(sanitized.tint_rgba, Some([1.0, 0.0, 0.5, 0.8]));
        assert!(sanitized.is_overlay);
        assert!(sanitized.is_clear);
    }
}
