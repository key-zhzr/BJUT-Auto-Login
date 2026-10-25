//! OS notifications supplement (never replace) the existing bounded polling.
use std::sync::{
    atomic::{AtomicI64, Ordering},
    Arc,
};
struct Context {
    app: tauri::AppHandle,
    state: Arc<crate::AppState>,
}
static LAST_EVENT: AtomicI64 = AtomicI64::new(0);
fn changed(context: &Context) {
    if !context.state.config.read().unwrap().wifi_change_detect {
        return;
    }
    let now = chrono::Utc::now().timestamp_millis();
    let last = LAST_EVENT.swap(now, Ordering::SeqCst);
    if now - last < 750 {
        return;
    }
    context.state.is_suspended.store(false, Ordering::SeqCst);
    context.state.non_campus_count.store(0, Ordering::SeqCst);
    crate::rust_log(
        &context.app,
        &context.state,
        "网络",
        "系统通知网络接口或路由发生变化，正在重新确认网络",
        "debug",
    );
    crate::schedule_network_change_readiness(context.app.clone(), context.state.clone());
}
pub(crate) fn start(app: tauri::AppHandle, state: Arc<crate::AppState>) {
    let context = Context { app, state };
    let _ = std::thread::Builder::new()
        .name("network-notifications".into())
        .spawn(move || platform::listen(context));
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::ffi::{c_char, c_void, CString};
    type Ref = *const c_void;
    #[repr(C)]
    struct StoreContext {
        version: isize,
        info: *mut c_void,
        retain: Option<unsafe extern "C" fn(Ref) -> Ref>,
        release: Option<unsafe extern "C" fn(Ref)>,
        description: Option<unsafe extern "C" fn(Ref) -> Ref>,
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(allocator: Ref, text: *const c_char, encoding: u32) -> Ref;
        fn CFArrayCreate(allocator: Ref, values: *const Ref, count: isize, callbacks: Ref) -> Ref;
        fn CFRelease(value: Ref);
        fn CFRunLoopGetCurrent() -> Ref;
        fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
        fn CFRunLoopRun();
        static kCFTypeArrayCallBacks: u8;
        static kCFRunLoopDefaultMode: Ref;
    }
    #[link(name = "SystemConfiguration", kind = "framework")]
    extern "C" {
        fn SCDynamicStoreCreate(
            allocator: Ref,
            name: Ref,
            callback: Option<unsafe extern "C" fn(Ref, Ref, *mut c_void)>,
            context: *mut StoreContext,
        ) -> Ref;
        fn SCDynamicStoreSetNotificationKeys(store: Ref, keys: Ref, patterns: Ref) -> u8;
        fn SCDynamicStoreCreateRunLoopSource(allocator: Ref, store: Ref, order: isize) -> Ref;
    }
    unsafe extern "C" fn callback(_: Ref, _: Ref, info: *mut c_void) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            changed(&*(info as *const Context))
        }));
    }
    pub(super) fn listen(mut context: Context) {
        unsafe {
            let text = CString::new("BJUT-AL network changes").unwrap();
            let name = CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), 0x08000100);
            let mut info = StoreContext {
                version: 0,
                info: (&mut context as *mut Context).cast(),
                retain: None,
                release: None,
                description: None,
            };
            let store = SCDynamicStoreCreate(std::ptr::null(), name, Some(callback), &mut info);
            CFRelease(name);
            if store.is_null() {
                return;
            }
            let strings = [
                "State:/Network/Global/IPv4",
                "State:/Network/Global/IPv6",
                "State:/Network/Global/DNS",
                "State:/Network/Interface/.*/IPv[46]",
                "State:/Network/Interface/.*/Link",
            ]
            .iter()
            .map(|s| {
                let text = CString::new(*s).unwrap();
                CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), 0x08000100)
            })
            .collect::<Vec<_>>();
            let callbacks = std::ptr::addr_of!(kCFTypeArrayCallBacks).cast();
            let keys = CFArrayCreate(std::ptr::null(), strings.as_ptr(), 3, callbacks);
            let patterns = CFArrayCreate(std::ptr::null(), strings[3..].as_ptr(), 2, callbacks);
            let installed = SCDynamicStoreSetNotificationKeys(store, keys, patterns) != 0;
            CFRelease(keys);
            CFRelease(patterns);
            for text in strings {
                CFRelease(text);
            }
            if installed {
                let source = SCDynamicStoreCreateRunLoopSource(std::ptr::null(), store, 0);
                if !source.is_null() {
                    CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopDefaultMode);
                    CFRunLoopRun();
                    CFRelease(source);
                }
            }
            CFRelease(store);
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    pub(super) fn listen(context: Context) {
        unsafe {
            let socket = libc::socket(libc::AF_NETLINK, libc::SOCK_RAW, libc::NETLINK_ROUTE);
            if socket < 0 {
                return;
            }
            let mut address: libc::sockaddr_nl = std::mem::zeroed();
            address.nl_family = libc::AF_NETLINK as u16;
            address.nl_groups = 1 | 0x10 | 0x100 | 0x40 | 0x400;
            if libc::bind(
                socket,
                (&address as *const libc::sockaddr_nl).cast(),
                std::mem::size_of_val(&address) as u32,
            ) < 0
            {
                libc::close(socket);
                return;
            }
            let mut buffer = [0u8; 8192];
            loop {
                let count = libc::recv(socket, buffer.as_mut_ptr().cast(), buffer.len(), 0);
                if count <= 0 {
                    break;
                }
                changed(&context);
            }
            libc::close(socket);
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use windows::Win32::{
        Foundation::HANDLE, NetworkManagement::IpHelper::*, Networking::WinSock::AF_UNSPEC,
    };
    unsafe extern "system" fn interface(
        ctx: *const c_void,
        _: *const MIB_IPINTERFACE_ROW,
        _: MIB_NOTIFICATION_TYPE,
    ) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            changed(&*(ctx as *const Context))
        }));
    }
    unsafe extern "system" fn address(
        ctx: *const c_void,
        _: *const MIB_UNICASTIPADDRESS_ROW,
        _: MIB_NOTIFICATION_TYPE,
    ) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            changed(&*(ctx as *const Context))
        }));
    }
    unsafe extern "system" fn route(
        ctx: *const c_void,
        _: *const MIB_IPFORWARD_ROW2,
        _: MIB_NOTIFICATION_TYPE,
    ) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            changed(&*(ctx as *const Context))
        }));
    }
    pub(super) fn listen(context: Context) {
        unsafe {
            // Windows owns callbacks until process exit. Keep the single context
            // alive for that lifetime; never leave a dangling stack pointer.
            let ptr = Box::into_raw(Box::new(context)).cast::<c_void>();
            let mut handles = [HANDLE::default(); 3];
            let results = [
                NotifyIpInterfaceChange(
                    AF_UNSPEC,
                    Some(interface),
                    Some(ptr.cast_const()),
                    false,
                    &mut handles[0],
                ),
                NotifyUnicastIpAddressChange(
                    AF_UNSPEC,
                    Some(address),
                    Some(ptr.cast_const()),
                    false,
                    &mut handles[1],
                ),
                NotifyRouteChange2(
                    AF_UNSPEC,
                    Some(route),
                    ptr.cast_const(),
                    false,
                    &mut handles[2],
                ),
            ];
            if results.iter().all(|result| result.0 != 0) {
                drop(Box::from_raw(ptr as *mut Context));
            }
        }
    }
}
