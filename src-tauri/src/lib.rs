mod billing;
mod billing_runtime;
mod campus_dns;
mod campus_services;
mod config_model;
mod cookie_jar;
mod dual_stack;
mod login_progress;
#[cfg(any(target_os = "macos", test))]
mod macos_network;
mod network_inventory;
mod network_platform;
mod network_probe;
mod network_progress;
mod network_repair;
mod network_schedule;
use network_repair::AdapterRestartTarget;
mod network_trust;
use campus_dns::{campus_dns_servers, query_campus_dns_ipv4};
use network_probe::{run_diagnostic_probes, NETWORK_PROBE_TIMEOUT};
mod connectivity;
mod diagnostic_paths;
mod internet_probe;
mod network_diagnostics;
mod network_events;
mod portal_auth;
mod recharge_state;
mod trusted_time;
mod update_metadata;
#[cfg(desktop)]
mod window_geometry;
#[cfg(target_os = "android")]
use internet_probe::check_internet_from_source;

use network_platform::*;

pub(crate) fn route_source_ipv4(destination: &str) -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect(destination)?;
            socket.local_addr()
        })
        .ok()
        .and_then(|address| address.is_ipv4().then(|| address.ip().to_string()))
        .unwrap_or_default()
}

pub(crate) fn usable_physical_ipv4(value: &str) -> Option<std::net::Ipv4Addr> {
    let address = value.trim().parse::<std::net::Ipv4Addr>().ok()?;
    let octets = address.octets();
    let is_fake_ip = octets[0] == 198 && matches!(octets[1], 18 | 19);
    if address.is_unspecified()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_multicast()
        || address.is_broadcast()
        || is_fake_ip
    {
        None
    } else {
        Some(address)
    }
}

fn network_ip_observation_changed(previous: Option<&str>, current: &str) -> bool {
    previous.is_some_and(|previous| previous != current)
}

#[cfg(target_os = "windows")]
#[derive(Default)]
struct WindowsNetworkIdentity {
    ssid: String,
    bssid: String,
    ip: String,
    interface_name: String,
    transport: String,
    wifi_identity_error: String,
}

#[cfg(target_os = "windows")]
#[derive(Default)]
struct WindowsWlanObservation {
    guid: String,
    ssid: String,
    bssid: String,
    error: String,
}

#[cfg(target_os = "windows")]
#[derive(Default)]
struct WindowsAdapterIdentity {
    guid: String,
    name: String,
    ip: String,
    interface_index: u32,
    operational: bool,
    has_gateway: bool,
    ipv4_metric: u32,
}

#[cfg(target_os = "windows")]
fn normalize_windows_guid(value: &str) -> String {
    value
        .trim()
        .trim_matches(|character| character == '{' || character == '}')
        .to_ascii_lowercase()
}

#[cfg(target_os = "windows")]
fn windows_guid_string(guid: &windows::core::GUID) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4[0],
        guid.data4[1],
        guid.data4[2],
        guid.data4[3],
        guid.data4[4],
        guid.data4[5],
        guid.data4[6],
        guid.data4[7]
    )
}

#[cfg(target_os = "windows")]
fn windows_wlan_observations(include_wifi_details: bool) -> Vec<WindowsWlanObservation> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::NetworkManagement::WiFi::{
        wlan_interface_state_connected, wlan_intf_opcode_current_connection, WlanCloseHandle,
        WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle, WlanQueryInterface,
        WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST,
    };

    let mut observations = Vec::new();
    let mut handle = HANDLE::default();
    let mut negotiated_version = 0u32;
    // SAFETY: all pointers are valid out-parameters and the handle/memory
    // returned by WLAN API are released before this function returns.
    let open_result = unsafe { WlanOpenHandle(2, None, &mut negotiated_version, &mut handle) };
    if open_result != 0 || handle.is_invalid() {
        return observations;
    }

    let mut interface_list = std::ptr::null_mut::<WLAN_INTERFACE_INFO_LIST>();
    // SAFETY: `handle` was opened above and interface_list is a valid
    // out-parameter initialized to null.
    let enum_result = unsafe { WlanEnumInterfaces(handle, None, &mut interface_list) };
    if enum_result == 0 && !interface_list.is_null() {
        // SAFETY: WLAN_INTERFACE_INFO_LIST is a C flexible-array structure;
        // the allocation contains dwNumberOfItems contiguous entries.
        let interfaces = unsafe {
            let list = &*interface_list;
            std::slice::from_raw_parts(list.InterfaceInfo.as_ptr(), list.dwNumberOfItems as usize)
        };
        for interface in interfaces {
            if interface.isState != wlan_interface_state_connected {
                continue;
            }
            let mut observation = WindowsWlanObservation {
                guid: windows_guid_string(&interface.InterfaceGuid),
                ..Default::default()
            };
            if include_wifi_details {
                let mut data_size = 0u32;
                let mut data = std::ptr::null_mut::<std::ffi::c_void>();
                // SAFETY: interface GUID comes from the same WLAN handle; the
                // returned buffer is checked for size and freed below.
                let query_result = unsafe {
                    WlanQueryInterface(
                        handle,
                        &interface.InterfaceGuid,
                        wlan_intf_opcode_current_connection,
                        None,
                        &mut data_size,
                        &mut data,
                        None,
                    )
                };
                if query_result == 0
                    && !data.is_null()
                    && data_size as usize >= std::mem::size_of::<WLAN_CONNECTION_ATTRIBUTES>()
                {
                    // SAFETY: successful query guarantees this buffer contains
                    // WLAN_CONNECTION_ATTRIBUTES and the size was checked.
                    let attributes = unsafe { &*(data.cast::<WLAN_CONNECTION_ATTRIBUTES>()) };
                    let association = &attributes.wlanAssociationAttributes;
                    let ssid_length = (association.dot11Ssid.uSSIDLength as usize)
                        .min(association.dot11Ssid.ucSSID.len());
                    observation.ssid =
                        String::from_utf8_lossy(&association.dot11Ssid.ucSSID[..ssid_length])
                            .trim()
                            .to_string();
                    observation.bssid = association
                        .dot11Bssid
                        .iter()
                        .map(|octet| format!("{octet:02x}"))
                        .collect::<Vec<_>>()
                        .join(":");
                } else if query_result == 5 {
                    // Windows 11 24H2 gates the current-connection query behind
                    // precise-location consent. Keep the same-interface IP and
                    // report the actionable reason instead of dropping all data.
                    observation.error = "locationPermissionDenied".to_string();
                } else {
                    observation.error = format!("wlanQueryFailed:{query_result}");
                }
                if !data.is_null() {
                    // SAFETY: data was allocated by WlanQueryInterface.
                    unsafe { WlanFreeMemory(data) };
                }
            }
            observations.push(observation);
        }
        // SAFETY: interface_list was allocated by WlanEnumInterfaces.
        unsafe { WlanFreeMemory(interface_list.cast()) };
    }
    // SAFETY: handle was returned by WlanOpenHandle and is no longer used.
    unsafe {
        WlanCloseHandle(handle, None);
    }
    observations
}

#[cfg(target_os = "windows")]
fn windows_interface_is_hardware(interface_index: u32) -> bool {
    use windows::Win32::Foundation::NO_ERROR;
    use windows::Win32::NetworkManagement::IpHelper::{GetIfEntry2, MIB_IF_ROW2};

    let mut row = MIB_IF_ROW2 {
        InterfaceIndex: interface_index,
        ..Default::default()
    };
    // SAFETY: row is an initialized in/out structure and remains valid for the
    // duration of the synchronous API call. HardwareInterface is the first
    // one-bit field in InterfaceAndOperStatusFlags.
    let result = unsafe { GetIfEntry2(&mut row) };
    result == NO_ERROR && row.InterfaceAndOperStatusFlags._bitfield & 1 != 0
}

#[cfg(target_os = "windows")]
fn windows_adapter_looks_virtual(name: &str, description: &str) -> bool {
    let normalized = format!("{name} {description}").to_ascii_lowercase();
    [
        "virtual",
        "hyper-v",
        "vmware",
        "virtualbox",
        "vethernet",
        "wintun",
        "wireguard",
        "tailscale",
        "zerotier",
        "tap-windows",
        "loopback",
        "bluetooth",
        "docker",
        "wsl",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

#[cfg(target_os = "windows")]
fn windows_physical_adapters(if_type: u32) -> Vec<WindowsAdapterIdentity> {
    use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST,
        GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

    let flags = GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_DNS_SERVER
        | GAA_FLAG_INCLUDE_GATEWAYS;
    let mut byte_count = 0u32;
    // SAFETY: the first call intentionally provides no buffer so Windows can
    // return the required allocation size in byte_count.
    let size_result =
        unsafe { GetAdaptersAddresses(AF_INET.0 as u32, flags, None, None, &mut byte_count) };
    if size_result != ERROR_BUFFER_OVERFLOW.0 || byte_count == 0 {
        return Vec::new();
    }

    // Use pointer-sized words rather than Vec<u8> so the Windows structures
    // are correctly aligned when the buffer is cast below.
    let word_size = std::mem::size_of::<usize>();
    let word_count = (byte_count as usize).div_ceil(word_size);
    let mut storage = vec![0usize; word_count];
    let first_adapter = storage.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    // SAFETY: storage is writable, suitably aligned and at least byte_count
    // bytes long; all linked records remain valid while storage is alive.
    let query_result = unsafe {
        GetAdaptersAddresses(
            AF_INET.0 as u32,
            flags,
            None,
            Some(first_adapter),
            &mut byte_count,
        )
    };
    if query_result != NO_ERROR.0 {
        return Vec::new();
    }

    let mut adapters = Vec::new();
    let mut adapter_ptr = first_adapter;
    while !adapter_ptr.is_null() {
        // SAFETY: adapter_ptr belongs to the linked list returned above.
        let adapter = unsafe { &*adapter_ptr };
        // SAFETY: this is the active member of the documented
        // Length/IfIndex header union.
        let interface_index = unsafe { adapter.Anonymous1.Anonymous.IfIndex };
        if adapter.IfType == if_type {
            // AdapterName is the stable interface GUID used by Native Wi-Fi;
            // FriendlyName is only for display.
            let guid = normalize_windows_guid(
                &unsafe { adapter.AdapterName.to_string() }.unwrap_or_default(),
            );
            let mut name = unsafe { adapter.FriendlyName.to_string() }.unwrap_or_default();
            let description = unsafe { adapter.Description.to_string() }.unwrap_or_default();
            let physical_address_length =
                (adapter.PhysicalAddressLength as usize).min(adapter.PhysicalAddress.len());
            let has_physical_address = physical_address_length >= 6
                && adapter.PhysicalAddress[..physical_address_length]
                    .iter()
                    .any(|octet| *octet != 0);
            // Some Windows releases and vendor NDIS drivers do not populate
            // HardwareInterface consistently. A real non-zero MAC on a
            // non-virtual Ethernet/WLAN adapter is the stable cross-version
            // fallback. Do not require OperStatus=Up here: several USB docks
            // expose an assigned unicast IPv4 while reporting Unknown until
            // after the first routed packet. A coherent per-interface IPv4
            // below remains mandatory; no global ipconfig guessing is used.
            if (!windows_interface_is_hardware(interface_index) && !has_physical_address)
                || windows_adapter_looks_virtual(&name, &description)
            {
                adapter_ptr = adapter.Next;
                continue;
            }
            if name.trim().is_empty() {
                name.clone_from(&description);
            }
            if name.trim().is_empty() {
                name.clone_from(&guid);
            }
            let mut address_ptr = adapter.FirstUnicastAddress;
            while !address_ptr.is_null() {
                // SAFETY: address_ptr is another linked record owned by storage.
                let address = unsafe { &*address_ptr };
                let socket_address = address.Address;
                if !socket_address.lpSockaddr.is_null()
                    && socket_address.iSockaddrLength as usize >= std::mem::size_of::<SOCKADDR_IN>()
                {
                    // SAFETY: GetAdaptersAddresses was restricted to AF_INET
                    // and the sockaddr length was checked above.
                    let ipv4 = unsafe { &*socket_address.lpSockaddr.cast::<SOCKADDR_IN>() };
                    if ipv4.sin_family == AF_INET {
                        // SAFETY: S_un_b is the byte view of the IPv4 union.
                        let octets = unsafe { ipv4.sin_addr.S_un.S_un_b };
                        let ip = std::net::Ipv4Addr::new(
                            octets.s_b1,
                            octets.s_b2,
                            octets.s_b3,
                            octets.s_b4,
                        )
                        .to_string();
                        if !name.is_empty() && usable_physical_ipv4(&ip).is_some() {
                            adapters.push(WindowsAdapterIdentity {
                                guid: guid.clone(),
                                name: name.clone(),
                                ip,
                                interface_index,
                                operational: adapter.OperStatus == IfOperStatusUp,
                                has_gateway: !adapter.FirstGatewayAddress.is_null(),
                                ipv4_metric: adapter.Ipv4Metric,
                            });
                        }
                    }
                }
                address_ptr = address.Next;
            }
        }
        adapter_ptr = adapter.Next;
    }
    adapters
}

#[cfg(target_os = "windows")]
fn windows_best_route_interface_index(destination: std::net::Ipv4Addr) -> Option<u32> {
    use windows::Win32::Foundation::NO_ERROR;
    use windows::Win32::NetworkManagement::IpHelper::{GetBestRoute2, MIB_IPFORWARD_ROW2};
    use windows::Win32::Networking::WinSock::SOCKADDR_INET;

    let destination = SOCKADDR_INET::from(std::net::SocketAddrV4::new(destination, 0));
    let mut route = MIB_IPFORWARD_ROW2::default();
    let mut source = SOCKADDR_INET::default();
    // SAFETY: all pointers refer to initialized stack values that outlive the
    // call. With no interface supplied, Windows reports the route it would
    // actually use for this Type 3 gateway.
    let result = unsafe { GetBestRoute2(None, 0, None, &destination, 0, &mut route, &mut source) };
    (result == NO_ERROR && route.InterfaceIndex != 0).then_some(route.InterfaceIndex)
}

#[cfg(target_os = "windows")]
fn windows_network_identity(include_wifi_details: bool, preferred: &str) -> WindowsNetworkIdentity {
    use windows::Win32::NetworkManagement::IpHelper::{IF_TYPE_ETHERNET_CSMACD, IF_TYPE_IEEE80211};

    let mut identity = WindowsNetworkIdentity::default();
    let observations = windows_wlan_observations(include_wifi_details);
    let wifi_adapters: Vec<_> = windows_physical_adapters(IF_TYPE_IEEE80211)
        .into_iter()
        .filter(|adapter| preferred.is_empty() || adapter.guid == preferred)
        .collect();
    let mut selected_wifi_metric = None;

    for adapter in &wifi_adapters {
        if let Some(observation) = observations
            .iter()
            .find(|observation| observation.guid == adapter.guid)
        {
            identity.ssid.clone_from(&observation.ssid);
            identity.bssid.clone_from(&observation.bssid);
            identity.wifi_identity_error.clone_from(&observation.error);
            identity.interface_name.clone_from(&adapter.name);
            identity.ip.clone_from(&adapter.ip);
            identity.transport = "wifi".to_string();
            selected_wifi_metric = Some(adapter.ipv4_metric);
            break;
        }
    }
    // Even when Windows denies location-sensitive SSID/BSSID access,
    // WlanEnumInterfaces still supplies the interface GUID. The native IP
    // Helper match above therefore retains a coherent Wi-Fi adapter/IP while
    // automatic credential submission fails closed on the missing identity.
    if identity.ip.is_empty() {
        if include_wifi_details && identity.wifi_identity_error.is_empty() {
            identity.wifi_identity_error = observations
                .first()
                .map(|observation| observation.error.clone())
                .filter(|error| !error.is_empty())
                .unwrap_or_else(|| "wlanInterfaceNotMatched".to_string());
        }
    }

    // Prefer the physical Ethernet adapter Windows would actually use for the
    // Type 3 gateway. This prevents simultaneous campus Wi-Fi from supplying
    // the URL address while Windows sends over its preferred wired route. If
    // the route is hidden by a TUN, choose an operational physical adapter by
    // gateway/metric (or the sole wired adapter). The read-only protocol probe
    // must still succeed before any credential is sent, so a normal home LAN
    // is observable in diagnostics without becoming automatically trusted.
    let wired_adapters: Vec<_> = windows_physical_adapters(IF_TYPE_ETHERNET_CSMACD)
        .into_iter()
        .filter(|adapter| preferred.is_empty() || adapter.guid == preferred)
        .collect();
    let routed_wired = [
        std::net::Ipv4Addr::new(10, 21, 251, 3),
        std::net::Ipv4Addr::new(10, 21, 221, 98),
        std::net::Ipv4Addr::new(172, 30, 201, 2),
    ]
    .into_iter()
    .filter_map(windows_best_route_interface_index)
    // A default VPN/TUN route may be returned for the first destination even
    // while a later campus subnet has a more specific physical route. Keep
    // evaluating destinations until the route index actually matches one of
    // the coherent physical Ethernet adapters enumerated above.
    .find_map(|index| {
        wired_adapters
            .iter()
            .find(|adapter| adapter.interface_index == index)
    });
    let campus_wired = wired_adapters
        .iter()
        .filter(|adapter| adapter.operational)
        .find(|adapter| is_campus_wired_ipv4(&adapter.ip))
        .or_else(|| {
            wired_adapters
                .iter()
                .find(|adapter| is_campus_wired_ipv4(&adapter.ip))
        });
    let best_active_wired = wired_adapters
        .iter()
        .filter(|adapter| adapter.operational)
        .min_by_key(|adapter| (!adapter.has_gateway, adapter.ipv4_metric))
        .or_else(|| {
            wired_adapters
                .iter()
                .filter(|adapter| adapter.has_gateway)
                .min_by_key(|adapter| adapter.ipv4_metric)
        })
        .or_else(|| (wired_adapters.len() == 1).then(|| &wired_adapters[0]));
    let metric_preferred_wired = best_active_wired.filter(|adapter| {
        identity.ip.is_empty()
            || selected_wifi_metric.is_none_or(|wifi_metric| adapter.ipv4_metric <= wifi_metric)
    });
    let wired = routed_wired.or(campus_wired).or(metric_preferred_wired);
    if let Some(adapter) = wired {
        identity.ssid.clear();
        identity.bssid.clear();
        identity.wifi_identity_error.clear();
        identity.interface_name.clone_from(&adapter.name);
        identity.ip.clone_from(&adapter.ip);
        identity.transport = "ethernet".to_string();
    }
    identity
}

fn preferred_interface_for_app(app: &tauri::AppHandle) -> String {
    app.try_state::<Arc<AppState>>()
        .map(|state| state.config.read().unwrap().preferred_interface.clone())
        .unwrap_or_default()
}

#[tauri::command]
fn get_link_health(state: tauri::State<Arc<AppState>>) -> Option<dual_stack::DualStackReport> {
    state.link_health.lock().unwrap().clone()
}

#[tauri::command]
fn get_network_adapters(app: tauri::AppHandle) -> serde_json::Value {
    let network = get_network_info(app.clone(), Some(false));
    let mut adapters = network_inventory::adapters();
    for adapter in &mut adapters {
        adapter.selected =
            network["interfaceName"].as_str() == Some(adapter.interface_name.as_str());
    }
    serde_json::json!({"adapters": adapters, "preferredInterface": preferred_interface_for_app(&app), "selectionSupported": !cfg!(target_os = "android")})
}

#[tauri::command]
fn set_preferred_interface(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    if cfg!(target_os = "android") {
        return Err("Android 由系统选择校园 Wi-Fi，当前仅展示网络接口".to_string());
    }
    if state.manual_login_in_progress.load(Ordering::SeqCst) {
        return Err("请等待当前登录或恢复操作结束后再选择网卡".to_string());
    }
    if !id.is_empty()
        && network_inventory::preferred_adapter(&network_inventory::adapters(), &id).is_none()
    {
        return Err("此网卡当前不可用于认证，请刷新网卡列表".to_string());
    }
    let mut config = state.config.read().unwrap().clone();
    let automatic = id.is_empty();
    config.preferred_interface = id;
    save_config(&app, &state, config)?;
    network_events::record(
        &app,
        &state,
        "selection",
        if automatic {
            "已选择自动识别认证网卡，优先校园有线"
        } else {
            "已手动指定认证网卡；该网卡断开时会等待恢复"
        },
    );
    schedule_network_change_readiness(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
fn get_network_info(
    _app: tauri::AppHandle,
    _include_wifi_details: Option<bool>,
) -> serde_json::Value {
    let preferred_interface = preferred_interface_for_app(&_app);
    #[cfg(target_os = "android")]
    let _ = &preferred_interface;
    #[cfg(target_os = "android")]
    {
        let mut result = serde_json::json!({
            "ssid": "",
            "bssid": "",
            "ip": "",
            "transport": "unknown",
            "validated": false,
            "metered": false,
            "wifiIdentityError": "networkInfoUnavailable"
        });
        if let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() {
            if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) } {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let activity =
                        unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };

                    match tauri::wry::prelude::find_class(
                        &mut env,
                        &activity,
                        "cn.edu.bjut.al.NetworkHelper".into(),
                    ) {
                        Ok(class) => {
                            let method_call = env.call_static_method(
                                class,
                                "getNetworkInfo",
                                "(Landroid/content/Context;Z)Ljava/lang/String;",
                                &[
                                    jni::objects::JValue::Object(&activity),
                                    jni::objects::JValue::Bool(
                                        if _include_wifi_details.unwrap_or(true) {
                                            1
                                        } else {
                                            0
                                        },
                                    ),
                                ],
                            );

                            match method_call {
                                Ok(jvalue) => {
                                    if let Ok(jobject) = jvalue.l() {
                                        let jstring: jni::objects::JString = jobject.into();
                                        if let Ok(rust_str) = env.get_string(&jstring).map(|s| {
                                            let s: String = s.into();
                                            s
                                        }) {
                                            if let Ok(val) = serde_json::from_str(&rust_str) {
                                                result = val;
                                            }
                                        }
                                    }
                                }
                                Err(_) => {
                                    if env.exception_check().unwrap_or(false) {
                                        let _ = env.exception_clear();
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            if env.exception_check().unwrap_or(false) {
                                let _ = env.exception_clear();
                            }
                        }
                    }

                    if env.exception_check().unwrap_or(false) {
                        let _ = env.exception_clear();
                    }
                }
            }
        }
        return result;
    }

    #[cfg(not(target_os = "android"))]
    {
        let mut ssid = String::new();
        let mut bssid = String::new();
        let mut ip = String::new();
        let mut interface_name = String::new();
        let mut identity_source = "unverifiedFallback".to_string();
        let mut transport = "unknown".to_string();
        #[cfg(target_os = "windows")]
        let wifi_identity_error: String;
        #[cfg(not(target_os = "windows"))]
        let wifi_identity_error = String::new();
        let route_ip = route_source_ipv4("10.21.251.3:80");
        #[cfg(target_os = "macos")]
        let mut lgn_wired_features = Vec::new();
        #[cfg(not(target_os = "macos"))]
        let lgn_wired_features: Vec<String> = Vec::new();

        #[cfg(target_os = "macos")]
        let mut lgn_link_configuration = serde_json::Value::Null;
        #[cfg(not(target_os = "macos"))]
        let lgn_link_configuration = serde_json::Value::Null;

        #[cfg(target_os = "macos")]
        if let Some(selected) = macos_network::physical_identity_for(&preferred_interface) {
            interface_name = selected.interface;
            ip = selected.ipv4;
            transport = selected.transport.to_string();
            identity_source = "sameInterface".to_string();
            if selected.transport == "wifi" && _include_wifi_details.unwrap_or(true) {
                if let Ok(client) = corewlan::WiFiClient::shared() {
                    if let Some(interface) = client.interface_with_name(Some(&interface_name)) {
                        if let Some(state) = _app.try_state::<Arc<AppState>>() {
                            rust_log(
                                &_app,
                                &state,
                                "隐私",
                                "[DEBUG] macOS 正在读取 SSID/BSSID；此操作可能显示系统位置使用指示",
                                "debug",
                            );
                        }
                        ssid = interface.ssid().unwrap_or_default();
                        bssid = interface.bssid().unwrap_or_default();
                    }
                }
            }
            if selected.transport == "ethernet" {
                let configuration = macos_network::lgn_link_configuration(&interface_name);
                lgn_wired_features = configuration.features(&ip);
                lgn_link_configuration = serde_json::to_value(configuration).unwrap_or_default();
            }
        }

        #[cfg(target_os = "windows")]
        {
            let identity = windows_network_identity(
                _include_wifi_details.unwrap_or(true),
                &preferred_interface,
            );
            ssid = identity.ssid;
            bssid = identity.bssid;
            ip = identity.ip;
            interface_name = identity.interface_name;
            transport = identity.transport;
            wifi_identity_error = identity.wifi_identity_error;
            if !ip.is_empty() && !interface_name.is_empty() {
                identity_source = "sameInterface".to_string();
            }
        }

        #[cfg(target_os = "linux")]
        {
            if let Ok(output) = std::process::Command::new("nmcli")
                .args(["-t", "-f", "device,active,ssid,bssid", "dev", "wifi"])
                .output()
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let parts = split_nmcli_fields(line);
                    if parts.len() >= 4 && parts[1] == "yes" {
                        interface_name = parts[0].clone();
                        ssid = parts[2].clone();
                        bssid = parts[3].clone();
                        if let Ok(output) = std::process::Command::new("ip")
                            .args([
                                "-4",
                                "-o",
                                "addr",
                                "show",
                                "dev",
                                &interface_name,
                                "scope",
                                "global",
                            ])
                            .output()
                        {
                            let output = String::from_utf8_lossy(&output.stdout);
                            if let Some(address) = output
                                .split_whitespace()
                                .find(|part| part.contains('.') && part.contains('/'))
                            {
                                ip = address.split('/').next().unwrap_or("").to_string();
                                if !ip.is_empty() {
                                    transport = "wifi".to_string();
                                    identity_source = "sameInterface".to_string();
                                }
                            }
                        }
                        break;
                    }
                }
            }

            let route_identity = linux_route_identity("172.30.201.2");
            if let Some((route_interface, route_ip)) =
                route_identity.filter(|(route_interface, route_ip)| {
                    route_interface != &interface_name
                        && linux_is_physical_ethernet_interface(route_interface)
                        && is_campus_wired_ipv4(route_ip)
                })
            {
                ssid.clear();
                bssid.clear();
                interface_name = route_interface;
                ip = route_ip;
                transport = "ethernet".to_string();
                identity_source = "sameInterface".to_string();
            } else if let Some((wired_interface, wired_ip)) =
                linux_campus_wired_identity(&interface_name)
            {
                // When a TUN device owns the route, the route query cannot
                // identify the physical campus Ethernet. Keep the scan
                // per-interface and prefer that campus address over Wi-Fi.
                ssid.clear();
                bssid.clear();
                interface_name = wired_interface;
                ip = wired_ip;
                transport = "ethernet".to_string();
                identity_source = "sameInterface".to_string();
            }
        }

        #[cfg(target_os = "linux")]
        if !preferred_interface.is_empty() {
            let adapters = network_inventory::adapters();
            if let Some(selected) =
                network_inventory::preferred_adapter(&adapters, &preferred_interface)
            {
                if interface_name != selected.interface_name {
                    ssid.clear();
                    bssid.clear();
                }
                interface_name.clone_from(&selected.interface_name);
                ip = selected.ipv4.first().cloned().unwrap_or_default();
                transport.clone_from(&selected.transport);
                identity_source = "sameInterface".to_string();
                if transport == "wifi" && _include_wifi_details.unwrap_or(true) {
                    if let Ok(output) = std::process::Command::new("nmcli")
                        .args([
                            "-t",
                            "-f",
                            "active,ssid,bssid",
                            "dev",
                            "wifi",
                            "list",
                            "ifname",
                            &interface_name,
                        ])
                        .output()
                    {
                        for line in String::from_utf8_lossy(&output.stdout).lines() {
                            let fields = split_nmcli_fields(line);
                            if fields.len() >= 3 && fields[0] == "yes" {
                                ssid = fields[1].clone();
                                bssid = fields[2].clone();
                                break;
                            }
                        }
                    }
                }
            } else {
                ssid.clear();
                bssid.clear();
                ip.clear();
                interface_name.clear();
                transport = "unknown".to_string();
                identity_source = "unavailable".to_string();
            }
        }

        serde_json::json!({
            "ssid": ssid,
            "bssid": bssid,
            "ip": ip,
            "interfaceName": interface_name,
            "identitySource": identity_source,
            "transport": transport,
            "routeIp": route_ip,
            "lgnWiredHint": transport.eq_ignore_ascii_case("ethernet") && is_lgn_wired_client_ipv4(&ip),
            "lgnWiredFeatures": lgn_wired_features,
            "lgnLinkConfiguration": lgn_link_configuration,
            "preferredInterface": preferred_interface,
            "wifiIdentityError": wifi_identity_error
        })
    }
}

#[cfg(target_os = "android")]
fn call_android_network_helper_bool(method: &str) -> bool {
    let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() else {
        return false;
    };
    let Ok(vm) = (unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) }) else {
        return false;
    };
    let Ok(mut env) = vm.attach_current_thread_as_daemon() else {
        return false;
    };
    let activity = unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
    let Ok(class) =
        tauri::wry::prelude::find_class(&mut env, &activity, "cn.edu.bjut.al.NetworkHelper".into())
    else {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        return false;
    };
    let result = env
        .call_static_method(
            class,
            method,
            "(Landroid/content/Context;)Z",
            &[jni::objects::JValue::Object(&activity)],
        )
        .and_then(|value| value.z())
        .unwrap_or(false);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
        return false;
    }
    result
}

#[cfg(target_os = "android")]
fn call_android_network_helper_string(method: &str) -> String {
    let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() else {
        return String::new();
    };
    let Ok(vm) = (unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) }) else {
        return String::new();
    };
    let Ok(mut env) = vm.attach_current_thread_as_daemon() else {
        return String::new();
    };
    let activity = unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
    let Ok(class) =
        tauri::wry::prelude::find_class(&mut env, &activity, "cn.edu.bjut.al.NetworkHelper".into())
    else {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        return String::new();
    };
    let result = env.call_static_method(
        class,
        method,
        "(Landroid/content/Context;)Ljava/lang/String;",
        &[jni::objects::JValue::Object(&activity)],
    );
    let output = result
        .ok()
        .and_then(|value| value.l().ok())
        .and_then(|object| {
            let value = jni::objects::JString::from(object);
            env.get_string(&value).ok().map(Into::into)
        })
        .unwrap_or_default();
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
        return String::new();
    }
    output
}

#[cfg(target_os = "android")]
fn call_android_network_helper_string_argument(method: &str, argument: &str) -> String {
    let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() else {
        return String::new();
    };
    let Ok(vm) = (unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) }) else {
        return String::new();
    };
    let Ok(mut env) = vm.attach_current_thread_as_daemon() else {
        return String::new();
    };
    let activity = unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
    let Ok(class) =
        tauri::wry::prelude::find_class(&mut env, &activity, "cn.edu.bjut.al.NetworkHelper".into())
    else {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        return String::new();
    };
    let Ok(java_argument) = env.new_string(argument) else {
        return String::new();
    };
    let java_argument = jni::objects::JObject::from(java_argument);
    let result = env.call_static_method(
        class,
        method,
        "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
        &[
            jni::objects::JValue::Object(&activity),
            jni::objects::JValue::Object(&java_argument),
        ],
    );
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
        return String::new();
    }
    result
        .ok()
        .and_then(|value| value.l().ok())
        .and_then(|object| {
            let value = jni::objects::JString::from(object);
            env.get_string(&value).ok().map(Into::into)
        })
        .unwrap_or_default()
}

#[cfg(target_os = "android")]
fn call_android_network_helper_bool_argument(method: &str, argument: &str) -> bool {
    let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() else {
        return false;
    };
    let Ok(vm) = (unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) }) else {
        return false;
    };
    let Ok(mut env) = vm.attach_current_thread_as_daemon() else {
        return false;
    };
    let activity = unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
    let Ok(class) =
        tauri::wry::prelude::find_class(&mut env, &activity, "cn.edu.bjut.al.NetworkHelper".into())
    else {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        return false;
    };
    let Ok(java_argument) = env.new_string(argument) else {
        return false;
    };
    let java_argument = jni::objects::JObject::from(java_argument);
    let result = env
        .call_static_method(
            class,
            method,
            "(Landroid/content/Context;Ljava/lang/String;)Z",
            &[
                jni::objects::JValue::Object(&activity),
                jni::objects::JValue::Object(&java_argument),
            ],
        )
        .and_then(|value| value.z())
        .unwrap_or(false);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
        return false;
    }
    result
}

#[cfg(target_os = "android")]
struct AndroidWifiRouteGuard {
    required: bool,
    token: Option<String>,
}

#[cfg(target_os = "android")]
impl AndroidWifiRouteGuard {
    fn bind_if_required(network: &serde_json::Value) -> Self {
        let required = network
            .get("routeBindingRequired")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let network_id = network
            .get("networkId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let token = required
            .then(|| {
                call_android_network_helper_string_argument(
                    "acquireCampusWifiBindingForNetwork",
                    network_id,
                )
            })
            .filter(|value| !value.is_empty());
        Self { required, token }
    }

    fn failed(&self) -> bool {
        self.required && self.token.is_none()
    }
}

#[cfg(target_os = "android")]
impl Drop for AndroidWifiRouteGuard {
    fn drop(&mut self) {
        if let Some(token) = self.token.take() {
            let _ =
                call_android_network_helper_bool_argument("releaseProcessNetworkBinding", &token);
        }
    }
}

#[cfg(target_os = "android")]
fn report_android_campus_wifi_connected(network: &serde_json::Value) {
    let network_id = network
        .get("wifiNetworkId")
        .or_else(|| network.get("networkId"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let _ = call_android_network_helper_bool_argument(
        "reportCampusWifiConnectivityForNetwork",
        network_id,
    );
}

#[cfg(target_os = "android")]
fn clear_android_network_binding() -> Result<(), String> {
    call_android_network_helper_bool("waitAndClearProcessNetworkBinding")
        .then_some(())
        .ok_or_else(|| "自动登录仍在使用校园 Wi-Fi 路由，请稍后重试".to_string())
}

#[tauri::command]
fn request_battery_optimizations(_app: tauri::AppHandle) {
    #[cfg(target_os = "android")]
    {
        if let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() {
            if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) } {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let activity =
                        unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };

                    let call =
                        env.call_method(&activity, "requestBatteryOptimizations", "()V", &[]);
                    if call.is_err() {
                        if env.exception_check().unwrap_or(false) {
                            let _ = env.exception_clear();
                        }
                    }
                }
            }
        }
    }
}

#[tauri::command]
fn request_foreground_permissions(_app: tauri::AppHandle) {
    #[cfg(target_os = "android")]
    {
        if let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() {
            if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) } {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let activity =
                        unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                    let _ = env.call_method(&activity, "requestForegroundPermissions", "()V", &[]);
                    if env.exception_check().unwrap_or(false) {
                        let _ = env.exception_clear();
                    }
                }
            }
        }
    }
}

#[tauri::command]
fn request_background_permissions(_app: tauri::AppHandle) {
    #[cfg(target_os = "android")]
    {
        if let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() {
            if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) } {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let activity =
                        unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                    let _ = env.call_method(&activity, "requestBackgroundPermissions", "()V", &[]);
                    if env.exception_check().unwrap_or(false) {
                        let _ = env.exception_clear();
                    }
                }
            }
        }
    }
}

#[tauri::command]
fn start_keep_alive_service(_app: tauri::AppHandle) {
    #[cfg(target_os = "android")]
    {
        if let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() {
            if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) } {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let activity =
                        unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                    let _ = env.call_method(&activity, "startKeepAliveService", "()V", &[]);
                    if env.exception_check().unwrap_or(false) {
                        let _ = env.exception_clear();
                    }
                }
            }
        }
    }
}

#[tauri::command]
fn stop_keep_alive_service(_app: tauri::AppHandle) {
    #[cfg(target_os = "android")]
    {
        if let Some(ctx) = tauri::tao::platform::android::prelude::main_android_context() {
            if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.java_vm.cast()) } {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let activity =
                        unsafe { jni::objects::JObject::from_raw(ctx.context_jobject.cast()) };
                    let _ = env.call_method(&activity, "stopKeepAliveService", "()V", &[]);
                    if env.exception_check().unwrap_or(false) {
                        let _ = env.exception_clear();
                    }
                }
            }
        }
    }
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn get_macos_dock_visible(state: tauri::State<Arc<AppState>>) -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        return state.config.read().ok()?.macos_dock_visible;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = state;
        None
    }
}

#[cfg(target_os = "macos")]
fn apply_macos_activation_policy(app: &tauri::AppHandle, visible: bool) -> Result<(), String> {
    use tauri::ActivationPolicy;
    app.set_activation_policy(if visible {
        ActivationPolicy::Regular
    } else {
        ActivationPolicy::Accessory
    })
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_dock_visible(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    visible: bool,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let snapshot = {
            let mut config = state.config.write().map_err(|error| error.to_string())?;
            config.macos_dock_visible = Some(visible);
            config.clone()
        };
        save_config(&app, &state, snapshot)?;
        apply_macos_activation_policy(&app, visible)?;
        if visible {
            // Keep the already-rendered window visible when switching from
            // Accessory to Regular. Avoid a delayed AppKit callback: it can
            // race with a close/hide event and used to refocus cold starts.
            if let Some(window) = app.get_webview_window("main") {
                if window.is_visible().unwrap_or(false) {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        }
    }
    let _ = app;
    let _ = state;
    let _ = visible;
    Ok(())
}

#[tauri::command]
fn frontend_ready(app: tauri::AppHandle, state: tauri::State<Arc<AppState>>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        #[cfg(desktop)]
        {
            window.set_focus().map_err(|e| e.to_string())?;
        }
    }
    // A hidden cold-start window is intentionally treated as background by
    // the initial 100 ms probe. Once the rendered frontend is shown, make the
    // foreground transition explicit and queue a full identity refresh. If a
    // cheap check is already running, trigger_network_check records a pending
    // full check instead of racing it.
    state.is_in_background.store(false, Ordering::SeqCst);
    let app_clone = app.clone();
    let state_clone = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        trigger_network_check(app_clone, state_clone, true).await;
    });
    Ok(())
}

#[tauri::command]
fn get_local_ip(app: tauri::AppHandle) -> String {
    let preferred = preferred_interface_for_app(&app);
    #[cfg(target_os = "android")]
    let _ = &preferred;
    #[cfg(target_os = "windows")]
    {
        return windows_network_identity(false, &preferred).ip;
    }

    #[cfg(target_os = "android")]
    {
        return call_android_network_helper_string("getPhysicalNetworkIp");
    }

    #[cfg(target_os = "macos")]
    {
        macos_network::physical_identity_for(&preferred)
            .map(|identity| identity.ipv4)
            .unwrap_or_default()
    }

    #[cfg(target_os = "linux")]
    {
        let _ = preferred;
        get_network_info(app, Some(false))["ip"]
            .as_str()
            .unwrap_or("")
            .to_string()
    }

    #[cfg(not(any(
        target_os = "windows",
        target_os = "android",
        target_os = "macos",
        target_os = "linux"
    )))]
    {
        String::new()
    }
}

#[tauri::command]
fn read_clipboard() -> Result<String, String> {
    #[cfg(not(target_os = "android"))]
    {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        clipboard.get_text().map_err(|e| e.to_string())
    }
    #[cfg(target_os = "android")]
    {
        Ok(String::new())
    }
}

#[tauri::command]
fn write_clipboard(text: String) -> Result<(), String> {
    #[cfg(not(target_os = "android"))]
    {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        clipboard.set_text(text).map_err(|e| e.to_string())
    }
    #[cfg(target_os = "android")]
    {
        let _ = text;
        Ok(())
    }
}

use config_model::{
    default_accent_color, default_android_notification_mode, default_balance_alert_threshold,
    default_color_mode, default_flow_alert_threshold, default_theme, default_vpn_compatibility,
    Account, AppConfig, NetworkProfile,
};
use network_trust::{
    evaluate_network_trust, is_campus_local_ip, is_known_campus_ssid, normalize_trust_lists,
    remove_network_trust, set_network_trust, NetworkTrustDecision, NetworkTrustInput,
};
#[cfg(test)]
use portal_auth::portal_probe_urls;
use portal_auth::{
    classify_portal_login_failure, detect_login_type_details_rust, diagnose_lgn_ipv6_rust,
    diagnose_login_gateways, lgn_user_info_url, login_result_is_ambiguous,
    login_to_campus_network_rust, logout_from_campus_network_rust, portal_client, LoginType,
    PortalLoginFailureDisposition, PortalRouteContext,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tauri::Emitter;
use tauri::Manager;

#[derive(serde::Serialize)]
struct ManualLoginResult {
    success: bool,
    message: String,
}

#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LoginSwitchContext {
    enabled: bool,
    current_account_user: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkTrustEvaluation {
    decision: String,
    reason: String,
    network_key: String,
    ssid: String,
    bssid: String,
    ip: String,
}

#[derive(serde::Serialize)]
struct NetworkTrustLists {
    whitelist: Vec<String>,
    blacklist: Vec<String>,
}

#[derive(serde::Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct UserInfo {
    account: String,
    balance: String,
    flow: String,
    flow_pending: bool,
    source: String,
    status: Option<String>,
    status_reason: Option<String>,
    package: Option<String>,
    package_detail: Option<String>,
    used_flow: Option<String>,
    billing_cycle: Option<String>,
    updated_at: String,
    billing_error: Option<String>,
    login_history: Vec<billing::BillingLoginRecord>,
    online_sessions: Vec<billing::BillingOnlineSession>,
    offline_tip: Option<String>,
    mauth_enabled: Option<bool>,
    billing_warnings: Vec<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateTarget {
    platform: String,
    arch: String,
    format: String,
    current_version: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct AccountHealth {
    #[serde(default)]
    consecutive_failures: u32,
    #[serde(default)]
    cooldown_until: Option<i64>,
    #[serde(default)]
    last_success: Option<String>,
    #[serde(default)]
    last_failure: Option<String>,
    #[serde(default)]
    last_failure_reason: Option<String>,
    #[serde(default)]
    failure_kind: Option<String>,
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct AccountHealthView {
    user: String,
    status: String,
    consecutive_failures: u32,
    cooldown_until: Option<i64>,
    cooldown_seconds: i64,
    last_success: Option<String>,
    last_failure: Option<String>,
    last_failure_reason: Option<String>,
    failure_kind: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CredentialStorageHealth {
    status: String,
    backend: String,
    persistent: bool,
    saved_accounts: usize,
    missing_password_accounts: Vec<String>,
    message: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EncryptedConfigBackup {
    version: u8,
    kdf: String,
    iterations: u32,
    salt: String,
    iv: String,
    ciphertext: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigBackupPlaintext {
    format: String,
    version: u8,
    config: AppConfig,
    #[serde(default)]
    scope: BackupScope,
    #[serde(default)]
    ui_preferences: serde_json::Value,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
struct BackupScope {
    settings: bool,
    accounts: bool,
}
impl Default for BackupScope {
    fn default() -> Self {
        Self {
            settings: true,
            accounts: true,
        }
    }
}
impl BackupScope {
    fn validate(self) -> Result<Self, String> {
        if self.settings || self.accounts {
            Ok(self)
        } else {
            Err("请至少选择设置或账号密码".into())
        }
    }
}

// Preserve current payment recovery records in every import. A backup cannot
// resurrect a historical payment or discard an outstanding current payment.
fn merge_backup_config(current: &AppConfig, imported: AppConfig, scope: BackupScope) -> AppConfig {
    let accounts = if scope.accounts {
        imported.accounts.clone()
    } else {
        current.accounts.clone()
    };
    let mut merged = if scope.settings {
        imported
    } else {
        current.clone()
    };
    merged.accounts = accounts;
    merged.campus_service_sessions = if scope.accounts {
        Default::default()
    } else {
        current.campus_service_sessions.clone()
    };
    merged.recharge_transactions = current.recharge_transactions.clone();
    merged
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigBackupExport {
    payload: String,
    account_count: usize,
    password_count: usize,
    missing_password_accounts: Vec<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigBackupImport {
    ui_preferences: serde_json::Value,
    account_count: usize,
    password_count: usize,
    missing_password_accounts: Vec<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticStep {
    id: String,
    label: String,
    status: String,
    message: String,
    duration_ms: u128,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticReport {
    stale: bool,
    created_at: String,
    overall: String,
    summary: String,
    ssid: String,
    ip: String,
    steps: Vec<DiagnosticStep>,
    adapter_restart: Option<AdapterRestartTarget>,
    dual_stack: dual_stack::DualStackReport,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct LogEntry {
    time: String,
    module: String,
    message: String,
    #[serde(rename = "type")]
    log_type: String, // "info" | "error" | "success" | "debug"
}

struct AppState {
    config: RwLock<AppConfig>,
    credential_storage_status: Mutex<String>,
    account_health: Mutex<HashMap<String, AccountHealth>>,
    logs: Mutex<Vec<LogEntry>>,
    countdown: AtomicI32,
    is_checking: AtomicBool,
    pending_full_check: AtomicBool,
    network_change_waiting: AtomicBool,
    network_change_generation: AtomicU64,
    login_operation_generation: AtomicU64,
    manual_login_in_progress: AtomicBool,
    login_progress: login_progress::LoginProgressControl,
    login_request_lock: tokio::sync::Mutex<()>,
    is_suspended: AtomicBool,
    last_known_ip: Mutex<Option<String>>,
    non_campus_count: AtomicU32,
    is_in_background: AtomicBool,
    last_network_state: Mutex<serde_json::Value>,
    link_health: Mutex<Option<dual_stack::DualStackReport>>,
    connectivity: connectivity::ProbePool,
    network_events: Mutex<network_events::Timeline>,
    system_online: AtomicBool,
    network_schedule: Mutex<network_schedule::AdaptiveSchedule>,
    network_progress: network_progress::Control,
    auto_login_paused_until: std::sync::atomic::AtomicI64,
    usage_alert_history: Mutex<HashMap<String, String>>,
    update_download: UpdateDownloadControl,
    billing_fetch_lock: tokio::sync::Mutex<()>,
    billing_sessions: Mutex<billing::BillingSessionPool>,
    billing_session_expiry_generation: AtomicU64,
    campus_service_lock: tokio::sync::Mutex<()>,
    campus_recharge_pending: tokio::sync::Mutex<Option<campus_services::PendingRecharge>>,
    campus_alipay_recharge_pending:
        tokio::sync::Mutex<Option<campus_services::PendingAlipayRecharge>>,
    campus_wechat_recharge_pending:
        tokio::sync::Mutex<Option<campus_services::PendingWechatRecharge>>,
    campus_wechat_payment_pending:
        tokio::sync::Mutex<Option<campus_services::PendingWechatPayment>>,
    pending_discovered_account: tokio::sync::Mutex<Option<PendingDiscoveredAccount>>,
}

#[derive(Default)]
struct UpdateDownloadControl {
    active: AtomicBool,
    paused: AtomicBool,
    cancelled: AtomicBool,
}

struct UpdateDownloadGuard<'a>(&'a UpdateDownloadControl);

struct ManualLoginOperationGuard<'a> {
    generation: &'a AtomicU64,
    active: &'a AtomicBool,
}

impl Drop for ManualLoginOperationGuard<'_> {
    fn drop(&mut self) {
        // Advance once more on completion. Checks that started during the
        // manual logout/login window therefore become stale as well.
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.active.store(false, Ordering::SeqCst);
    }
}

fn network_check_login_operation_superseded(state: &AppState, generation: u64) -> bool {
    state.manual_login_in_progress.load(Ordering::SeqCst)
        || state.login_operation_generation.load(Ordering::SeqCst) != generation
}

impl UpdateDownloadControl {
    fn begin(&self) -> Result<UpdateDownloadGuard<'_>, String> {
        self.active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "已有更新包正在下载".to_string())?;
        self.paused.store(false, Ordering::SeqCst);
        self.cancelled.store(false, Ordering::SeqCst);
        Ok(UpdateDownloadGuard(self))
    }
}

impl Drop for UpdateDownloadGuard<'_> {
    fn drop(&mut self) {
        self.0.paused.store(false, Ordering::SeqCst);
        self.0.cancelled.store(false, Ordering::SeqCst);
        self.0.active.store(false, Ordering::SeqCst);
    }
}

struct PendingDiscoveredAccount {
    account: Account,
    token: String,
    expires_at: std::time::Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VpnCompatibility {
    /// HTTPS and the operating system resolver.
    Minimum,
    /// HTTPS, with the campus DNS server used to obtain reqwest overrides.
    Low,
    /// HTTPS, with known portal addresses pinned in reqwest while preserving SNI.
    High,
    /// Direct HTTP requests to the portal IP addresses.
    Maximum,
}

impl VpnCompatibility {
    fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "minimum" | "min" | "lowest" => Self::Minimum,
            "low" | "lower" => Self::Low,
            "maximum" | "max" | "highest" => Self::Maximum,
            _ => Self::High,
        }
    }

    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    fn as_str(self) -> &'static str {
        match self {
            Self::Minimum => "minimum",
            Self::Low => "low",
            Self::High => "high",
            Self::Maximum => "maximum",
        }
    }
}

fn effective_vpn_compatibility(config: &AppConfig) -> VpnCompatibility {
    let configured = VpnCompatibility::from_config(&config.vpn_compatibility);
    if configured == VpnCompatibility::Maximum
        && config
            .vpn_maximum_until
            .is_some_and(|until| until > chrono::Utc::now().timestamp())
    {
        VpnCompatibility::Maximum
    } else if configured == VpnCompatibility::Maximum {
        VpnCompatibility::High
    } else {
        configured
    }
}

const TYPE1_MAXIMUM_MODE_REQUIRED_MESSAGE: &str =
    "已定位 bjut-sushe 认证网关，但未找到可通过证书与协议校验的 HTTPS:802 域名入口。应用不会忽略证书错误或静默发送明文密码；请确认 Wi-Fi 可信后，在设置中临时启用“最高兼容（HTTP + IP）”。";

fn type1_portal_requires_maximum(
    detection: &portal_auth::LoginTypeDetection,
    compatibility: VpnCompatibility,
) -> bool {
    detection.login_type == LoginType::Type1
        && detection.portal_detected
        && !detection.login_ready
        && compatibility != VpnCompatibility::Maximum
}

const MOBILE_DATA_CHECK_INTERVAL_FOREGROUND: i32 = 120;
const MOBILE_DATA_CHECK_INTERVAL_BACKGROUND: i32 = 300;

fn network_transport(network: &serde_json::Value) -> &str {
    network
        .get("transport")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
}

fn is_mobile_data_network(network: &serde_json::Value) -> bool {
    #[cfg(target_os = "android")]
    {
        return network_transport(network).eq_ignore_ascii_case("cellular");
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = network;
        false
    }
}

fn network_identity_is_fresh(network: &serde_json::Value) -> bool {
    #[cfg(target_os = "android")]
    {
        return network
            .get("identityFresh")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
    }
    #[cfg(not(target_os = "android"))]
    {
        network
            .get("identitySource")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|source| source.eq_ignore_ascii_case("sameInterface"))
    }
}

fn portal_route_context_from_network(
    network: &serde_json::Value,
) -> Result<Option<PortalRouteContext>, String> {
    #[cfg(target_os = "android")]
    {
        // Android's Network object is still bound by AndroidWifiRouteGuard (or
        // by KeepAliveService for the headless core). The route context here is
        // only the coherent physical interface/IP pair used to build portal
        // parameters (and the read-only probe URL); portal_client deliberately
        // does not replace the exact Network binding with an interface socket
        // option.
        let interface_name = network
            .get("interfaceName")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let physical_ipv4 = network
            .get("ip")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        return PortalRouteContext::new(interface_name, physical_ipv4).map(Some);
    }

    #[cfg(not(target_os = "android"))]
    {
        if !network_identity_is_fresh(network) {
            return Err("网络身份不是来自同一物理接口，已停止校园网网关请求".to_string());
        }
        let interface_name = network
            .get("interfaceName")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let physical_ipv4 = network
            .get("ip")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        PortalRouteContext::new(interface_name, physical_ipv4).map(Some)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NetworkIdentitySnapshot {
    network_id: String,
    interface_name: String,
    transport: String,
    ssid: String,
    bssid: String,
    ip: String,
}

impl NetworkIdentitySnapshot {
    fn capture(network: &serde_json::Value) -> Self {
        let component = |name: &str| {
            network
                .get(name)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let raw_ssid = component("ssid");
        let exact_ssid = raw_ssid
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(&raw_ssid)
            .to_string();
        Self {
            network_id: component("networkId"),
            interface_name: component("interfaceName"),
            transport: component("transport").to_ascii_lowercase(),
            // SSIDs are case-sensitive byte strings. Trust evaluation may
            // normalize spelling, but a TOCTOU guard must compare the exact
            // observed identity and must not equate `_` with `-`.
            ssid: exact_ssid,
            bssid: component("bssid").to_ascii_lowercase().replace('-', ":"),
            ip: component("ip"),
        }
    }

    fn changed_fields(&self, current: &Self) -> Vec<&'static str> {
        let mut changed = Vec::new();
        if self.network_id != current.network_id {
            changed.push("Network");
        }
        if self.interface_name != current.interface_name {
            changed.push("网络接口");
        }
        if self.transport != current.transport {
            changed.push("连接类型");
        }
        if self.ssid != current.ssid {
            changed.push("SSID");
        }
        if self.bssid != current.bssid {
            changed.push("BSSID");
        }
        if self.ip != current.ip {
            changed.push("IP");
        }
        changed
    }
}

fn ensure_same_network_identity(
    expected: &NetworkIdentitySnapshot,
    current_network: &serde_json::Value,
) -> Result<(), String> {
    let current = NetworkIdentitySnapshot::capture(current_network);
    let changed = expected.changed_fields(&current);
    if changed.is_empty() {
        Ok(())
    } else {
        Err(format!("{} 已变化", changed.join("、")))
    }
}

#[cfg(not(target_os = "android"))]
fn same_exact_wifi_identity(
    expected_ssid: &str,
    expected_bssid: &str,
    current_ssid: &str,
    current_bssid: &str,
) -> bool {
    let expected = NetworkIdentitySnapshot::capture(&serde_json::json!({
        "ssid": expected_ssid,
        "bssid": expected_bssid,
    }));
    let current = NetworkIdentitySnapshot::capture(&serde_json::json!({
        "ssid": current_ssid,
        "bssid": current_bssid,
    }));
    !current.ssid.is_empty()
        && !current.bssid.is_empty()
        && expected.ssid == current.ssid
        && expected.bssid == current.bssid
}

fn mobile_data_check_interval(configured: i32, is_background: bool) -> i32 {
    configured.max(if is_background {
        MOBILE_DATA_CHECK_INTERVAL_BACKGROUND
    } else {
        MOBILE_DATA_CHECK_INTERVAL_FOREGROUND
    })
}

fn automatic_login_network_allowed(input: NetworkTrustInput<'_>) -> Result<(), String> {
    if !input.identity_fresh {
        return Err("网络身份不是来自同一物理接口，已阻止自动发送账号密码".to_string());
    }
    let result = evaluate_network_trust(input);
    match result.decision {
        NetworkTrustDecision::Allowed => Ok(()),
        NetworkTrustDecision::Blocked | NetworkTrustDecision::NeedsConfirmation => {
            Err(result.reason)
        }
    }
}

fn login_type_from_profile(value: &str) -> Option<LoginType> {
    match value {
        "bjut-sushe" | "bjut_sushe" | "type1" => Some(LoginType::Type1),
        "bjut-wifi" | "bjut_wifi" | "type2" => Some(LoginType::Type2),
        "wired" | "lgn-wired" | "type3" => Some(LoginType::Type3),
        _ => None,
    }
}

fn parse_login_type_override(value: Option<&str>) -> Option<LoginType> {
    match value {
        Some("bjut-sushe") | Some("bjut_sushe") => Some(LoginType::Type1),
        Some("bjut-wifi") | Some("bjut_wifi") => Some(LoginType::Type2),
        Some("wired") | Some("lgn-wired") => Some(LoginType::Type3),
        _ => None,
    }
}

fn matching_network_profile(
    config: &AppConfig,
    ssid: &str,
    bssid: &str,
    detected_type: &LoginType,
) -> Option<NetworkProfile> {
    config
        .network_profiles
        .iter()
        .find(|profile| {
            if !profile.enabled {
                return false;
            }
            let ssid_matches = if profile.ssid.trim().is_empty() {
                *detected_type == LoginType::Type3
                    && (ssid.trim().is_empty()
                        || ssid.eq_ignore_ascii_case("unknown")
                        || ssid.eq_ignore_ascii_case("<unknown ssid>"))
            } else {
                profile.ssid.trim().eq_ignore_ascii_case(ssid.trim())
            };
            let bssid_matches = profile.bssid.trim().is_empty()
                || profile.bssid.trim().eq_ignore_ascii_case(bssid.trim());
            ssid_matches && bssid_matches
        })
        .cloned()
}

fn accounts_for_profile(accounts: Vec<Account>, profile: Option<&NetworkProfile>) -> Vec<Account> {
    let active: Vec<Account> = accounts
        .into_iter()
        .filter(|account| !account.is_disabled.unwrap_or(false) && !account.pass.is_empty())
        .collect();
    let Some(profile) = profile else {
        return active;
    };
    if profile.account_order.is_empty() {
        return active;
    }
    profile
        .account_order
        .iter()
        .filter_map(|user| active.iter().find(|account| account.user == *user).cloned())
        .collect()
}

fn profile_auto_login_enabled(
    profile: Option<&NetworkProfile>,
    login_type: &LoginType,
    global_default: bool,
) -> bool {
    let Some(profile) = profile else {
        return global_default;
    };
    let legacy_default = profile.auto_login.unwrap_or(global_default);
    let key = match login_type {
        LoginType::Type1 => "type1",
        LoginType::Type2 => "type2",
        LoginType::Type3 => "type3",
        LoginType::Unknown => return legacy_default,
    };
    profile
        .auto_login_types
        .get(key)
        .copied()
        .unwrap_or(legacy_default)
}

#[allow(unused_variables)]
fn app_is_in_background(app: &tauri::AppHandle, state: &AppState) -> bool {
    let reported_background = state.is_in_background.load(Ordering::SeqCst);
    #[cfg(desktop)]
    if let Some(window) = app.get_webview_window("main") {
        let visible = window.is_visible().unwrap_or(false);
        let focused = window.is_focused().unwrap_or(false);
        let minimized = window.is_minimized().unwrap_or(false);
        let window_background = desktop_window_background_state(
            visible,
            focused,
            minimized,
            !cfg!(target_os = "windows"),
        );
        // On Windows, WebView2 focus reporting can remain false even while the
        // native window is visible and interactive. Derive the state directly
        // from visibility/minimization so a stale WebView report cannot latch
        // the whole app in background mode.
        return if cfg!(target_os = "windows") {
            window_background
        } else {
            reported_background || window_background
        };
    }
    reported_background
}

fn desktop_window_background_state(
    visible: bool,
    focused: bool,
    minimized: bool,
    unfocused_counts_as_background: bool,
) -> bool {
    !visible || minimized || (unfocused_counts_as_background && !focused)
}

fn ensure_billing_foreground(state: &AppState) -> Result<(), String> {
    billing_runtime::ensure_foreground(&state.is_in_background)
}

async fn run_billing_read_while_foreground<T, F>(state: &AppState, future: F) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>>,
{
    billing_runtime::run_read_while_foreground(&state.is_in_background, future).await
}

async fn run_billing_mutation_to_completion<T, F>(state: &AppState, future: F) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>>,
{
    billing_runtime::run_mutation_to_completion(&state.is_in_background, future).await
}

const WLGN_HOST: &str = "wlgn.bjut.edu.cn";
const LGN_HOST: &str = "lgn.bjut.edu.cn";
const LGN6_HOST: &str = "lgn6.bjut.edu.cn";

#[cfg(target_os = "android")]
#[derive(serde::Serialize)]
struct HeadlessLog {
    module: String,
    message: String,
    #[serde(rename = "type")]
    log_type: String,
}

#[cfg(target_os = "android")]
fn headless_log(
    logs: &mut Vec<HeadlessLog>,
    module: &str,
    message: impl Into<String>,
    log_type: &str,
) {
    logs.push(HeadlessLog {
        module: module.to_string(),
        message: message.into(),
        log_type: log_type.to_string(),
    });
}

#[cfg(target_os = "android")]
async fn run_headless_network_check(
    config: AppConfig,
    network: serde_json::Value,
    mut account_health: HashMap<String, AccountHealth>,
    reason: &str,
) -> serde_json::Value {
    let mut logs = Vec::new();
    let compatibility = effective_vpn_compatibility(&config);
    let transport = network_transport(&network).to_string();
    let validated = network
        .get("validated")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let identity_requested = network
        .get("identityRequested")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let identity_fresh = network
        .get("identityFresh")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let ssid = network
        .get("ssid")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let bssid = network
        .get("bssid")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let ip = network
        .get("ip")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if config.log_level == "debug" {
        headless_log(
            &mut logs,
            "Android后台",
            format!(
                "[DEBUG] 无界面检测开始：来源={reason}，传输={transport}，系统验证={validated}，VPN兼容={}",
                compatibility.as_str()
            ),
            "debug",
        );
    }

    if transport.eq_ignore_ascii_case("cellular") {
        let online = check_internet_from_source(Some(&ip)).await;
        headless_log(
            &mut logs,
            "网络",
            if online {
                "App 自身探测确认移动数据已连通；已放缓后台检测并停止校园网网关探测"
            } else {
                "App 自身探测未确认移动数据连通；已放缓后台检测并停止校园网网关探测"
            },
            "info",
        );
        return serde_json::json!({
            "status": if online { "online" } else { "cellular" },
            "notification_category": "network",
            "notification": if online { "移动数据已连接，校园网探测已暂停" } else { "移动数据连通性暂未确认" },
            "logs": logs,
        });
    }

    if check_internet_from_source(Some(&ip)).await {
        headless_log(&mut logs, "网络", "无界面检测完成：互联网已连通", "info");
        return serde_json::json!({
            "status": "online",
            "notification_category": "network",
            "notification": "后台检测正常，互联网已连接",
            "logs": logs,
        });
    }

    if transport.eq_ignore_ascii_case("wifi") && !identity_fresh && is_campus_local_ip(&ip) {
        let (status, notification, message_type) = if identity_requested {
            (
                "blocked",
                "无法确认当前 Wi-Fi 身份，已阻止自动登录",
                "error",
            )
        } else {
            (
                "needs_fresh_identity",
                "检测到校园网地址，准备核对当前 Wi-Fi 身份",
                "debug",
            )
        };
        headless_log(
            &mut logs,
            "安全",
            if identity_requested {
                "系统未能返回有效的当前 SSID/BSSID，已阻止发送账号密码"
            } else {
                "发送凭据前需要读取一次当前 SSID/BSSID"
            },
            message_type,
        );
        return serde_json::json!({
            "status": status,
            "notification_category": if identity_requested { "login" } else { "network" },
            "notification": notification,
            "logs": logs,
        });
    }

    // Keep the physical IPv4 that belongs to the Android Network object with
    // the headless probe.  Type 1 loadConfig expects the same Base64-encoded
    // address the portal page supplies; omitting it can make a reachable
    // gateway look absent during background automatic login.
    let portal_route_context = portal_route_context_from_network(&network).ok().flatten();
    let detection = detect_login_type_details_rust(
        compatibility,
        &ssid,
        &transport,
        portal_route_context.as_ref(),
    )
    .await;
    if type1_portal_requires_maximum(&detection, compatibility) {
        headless_log(
            &mut logs,
            "网络",
            TYPE1_MAXIMUM_MODE_REQUIRED_MESSAGE,
            "info",
        );
        return serde_json::json!({
            "status": "campus",
            "notification_category": "network",
            "notification": "已发现宿舍网认证入口，需要确认 HTTP 兼容模式",
            "logs": logs,
        });
    }
    let detected = if detection.login_ready {
        detection.login_type.clone()
    } else {
        LoginType::Unknown
    };
    if detected != LoginType::Unknown && transport.eq_ignore_ascii_case("wifi") && !identity_fresh {
        let status = if identity_requested {
            "blocked"
        } else {
            "needs_fresh_identity"
        };
        headless_log(
            &mut logs,
            "安全",
            if identity_requested {
                "校园网认证网关可达，但无法确认当前 Wi-Fi 身份；已阻止发送账号密码"
            } else {
                "校园网认证网关可达；发送凭据前需要读取一次当前 SSID/BSSID"
            },
            if identity_requested { "error" } else { "debug" },
        );
        return serde_json::json!({
            "status": status,
            "notification_category": if identity_requested { "login" } else { "network" },
            "notification": if identity_requested {
                "无法确认当前 Wi-Fi 身份，已阻止自动登录"
            } else {
                "准备核对当前 Wi-Fi 身份"
            },
            "logs": logs,
        });
    }
    let profile = matching_network_profile(&config, &ssid, &bssid, &detected);
    let login_type = profile
        .as_ref()
        .and_then(|item| login_type_from_profile(&item.login_type))
        .unwrap_or(detected);
    if login_type == LoginType::Unknown {
        headless_log(
            &mut logs,
            "网络",
            "无界面检测未找到可访问的校园网认证网关",
            "info",
        );
        return serde_json::json!({
            "status": "offline",
            "notification_category": "network",
            "notification": "网络离线或不在校园网环境",
            "logs": logs,
        });
    }

    if !profile_auto_login_enabled(profile.as_ref(), &login_type, config.auto_login) {
        headless_log(
            &mut logs,
            "网络",
            "已检测到校园网认证网关，但当前协议的自动登录已停用",
            "info",
        );
        return serde_json::json!({
            "status": "campus",
            "notification_category": "network",
            "notification": "校园网需要认证，自动登录已停用",
            "logs": logs,
        });
    }
    if let Err(reason) = automatic_login_network_allowed(NetworkTrustInput {
        login_type: &login_type,
        ssid: &ssid,
        bssid: &bssid,
        ip: &ip,
        transport: &transport,
        identity_fresh,
        whitelist: &config.whitelist,
        blacklist: &config.blacklist,
    }) {
        headless_log(
            &mut logs,
            "安全",
            format!("无界面自动登录已阻止：{reason}"),
            "error",
        );
        return serde_json::json!({
            "status": "blocked",
            "notification_category": "login",
            "notification": "校园网登录被安全策略阻止",
            "logs": logs,
        });
    }

    let accounts = accounts_for_profile(config.accounts.clone(), profile.as_ref());
    let mut active_accounts = Vec::new();
    for account in accounts {
        let remaining = account_health
            .get(&account.user)
            .and_then(|item| item.cooldown_until)
            .map(|until| until - chrono::Utc::now().timestamp())
            .unwrap_or(0);
        if remaining > 0 {
            headless_log(
                &mut logs,
                "账号健康",
                format!(
                    "账号 {} 仍在冷却中（剩余 {} 秒），无界面核心已跳过",
                    account.user, remaining
                ),
                "info",
            );
        } else {
            active_accounts.push(account);
        }
    }
    if active_accounts.is_empty() {
        headless_log(
            &mut logs,
            "网络",
            "没有可供无界面自动登录使用的账号（可能均处于冷却或未保存密码）",
            "error",
        );
        return serde_json::json!({
            "status": "campus",
            "notification_category": "login",
            "notification": "校园网需要认证，但当前没有可尝试账号",
            "accountHealth": account_health,
            "logs": logs,
        });
    }

    let mut last_message = "所有账号均登录失败".to_string();
    for account in active_accounts {
        headless_log(
            &mut logs,
            "网络",
            format!("无界面核心尝试使用账号 {} 自动登录", account.user),
            "info",
        );
        match login_to_campus_network_rust(
            login_type.clone(),
            &account.user,
            &account.pass,
            compatibility,
            portal_route_context.as_ref(),
        )
        .await
        {
            Ok((true, message)) => {
                let item = account_health.entry(account.user.clone()).or_default();
                item.consecutive_failures = 0;
                item.cooldown_until = None;
                item.last_success =
                    Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                item.last_failure_reason = None;
                item.failure_kind = None;
                headless_log(
                    &mut logs,
                    "网络",
                    format!("无界面自动登录成功：账号 {}，{message}", account.user),
                    "success",
                );
                return serde_json::json!({
                    "status": "login_success",
                    "notification_category": "login",
                    "notification": format!("校园网自动登录成功：{}", account.user),
                    "accountHealth": account_health,
                    "logs": logs,
                });
            }
            Ok((false, message)) => {
                last_message = message.clone();
                let item = account_health.entry(account.user.clone()).or_default();
                item.consecutive_failures = item.consecutive_failures.saturating_add(1);
                let (kind, cooldown_seconds) =
                    classify_account_failure(&message, item.consecutive_failures);
                item.cooldown_until = Some(chrono::Utc::now().timestamp() + cooldown_seconds);
                item.last_failure =
                    Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                item.last_failure_reason = Some(message.clone());
                item.failure_kind = Some(kind.to_string());
                headless_log(
                    &mut logs,
                    "网络",
                    format!("无界面自动登录失败：账号 {}，{message}", account.user),
                    "error",
                );
            }
            Err(error) => {
                last_message = error.clone();
                if login_result_is_ambiguous(&error) {
                    headless_log(
                        &mut logs,
                        "网络",
                        format!(
                            "无界面登录结果无法确认：账号 {}，{}；本轮不再尝试其他账号",
                            account.user, error
                        ),
                        "error",
                    );
                    return serde_json::json!({
                        "status": "login_unknown",
                        "notification_category": "login",
                        "notification": "校园网登录请求已发送，但结果无法确认；请勿立即重试",
                        "accountHealth": account_health,
                        "logs": logs,
                    });
                }
                let item = account_health.entry(account.user.clone()).or_default();
                item.consecutive_failures = item.consecutive_failures.saturating_add(1);
                let reason = format!("请求出错: {error}");
                let (kind, cooldown_seconds) =
                    classify_account_failure(&reason, item.consecutive_failures);
                item.cooldown_until = Some(chrono::Utc::now().timestamp() + cooldown_seconds);
                item.last_failure =
                    Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                item.last_failure_reason = Some(reason);
                item.failure_kind = Some(kind.to_string());
                headless_log(
                    &mut logs,
                    "网络",
                    format!("无界面登录请求失败：账号 {}，{error}", account.user),
                    "error",
                );
            }
        }
    }
    serde_json::json!({
        "status": "login_failed",
        "notification_category": "login",
        "notification": format!("校园网自动登录失败：{last_message}"),
        "accountHealth": account_health,
        "logs": logs,
    })
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_cn_edu_bjut_al_NativeKeepAlive_runHeadlessCheck(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    config_json: jni::objects::JString,
    network_info_json: jni::objects::JString,
    account_health_json: jni::objects::JString,
    reason: jni::objects::JString,
) -> jni::sys::jstring {
    let result = (|| -> Result<String, String> {
        let config_json: String = env
            .get_string(&config_json)
            .map_err(|error| error.to_string())?
            .into();
        let network_info_json: String = env
            .get_string(&network_info_json)
            .map_err(|error| error.to_string())?
            .into();
        let account_health_json: String = env
            .get_string(&account_health_json)
            .map_err(|error| error.to_string())?
            .into();
        let reason: String = env
            .get_string(&reason)
            .map_err(|error| error.to_string())?
            .into();
        let config: AppConfig = serde_json::from_str(&config_json)
            .map_err(|error| format!("解析安全配置失败：{error}"))?;
        let network: serde_json::Value = serde_json::from_str(&network_info_json)
            .map_err(|error| format!("解析网络信息失败：{error}"))?;
        let account_health: HashMap<String, AccountHealth> =
            serde_json::from_str(&account_health_json).unwrap_or_default();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("创建无界面运行时失败：{error}"))?;
        let mut payload = runtime.block_on(run_headless_network_check(
            config.clone(),
            network.clone(),
            account_health,
            &reason,
        ));
        static SCHEDULE: std::sync::OnceLock<Mutex<network_schedule::AdaptiveSchedule>> =
            std::sync::OnceLock::new();
        let mut schedule = SCHEDULE
            .get_or_init(|| Mutex::new(network_schedule::AdaptiveSchedule::default()))
            .lock()
            .unwrap();
        let identity = format!("{}|{}", network["networkId"], network["ip"]);
        let plan = schedule.observe(network_schedule::ScheduleInput {
            identity: &identity,
            online: matches!(payload["status"].as_str(), Some("online" | "login_success")),
            has_link: !network["ip"].as_str().unwrap_or("").is_empty(),
            background: true,
            mobile_data: is_mobile_data_network(&network),
            power_saving: network["powerSaving"].as_bool().unwrap_or(false),
            enabled: config.adaptive_network_checks,
            configured: config.check_interval_bg,
        });
        payload["schedule"] = serde_json::to_value(plan).unwrap_or_default();
        serde_json::to_string(&payload).map_err(|error| error.to_string())
    })()
    .unwrap_or_else(|error| {
        serde_json::json!({
            "status": "error",
            "notification_category": "background",
            "notification": "后台检测核心启动失败",
            "logs": [{
                "module": "Android后台",
                "message": error,
                "type": "error"
            }]
        })
        .to_string()
    });
    env.new_string(result)
        .map(|value| value.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

fn redact_request_error(error: reqwest::Error) -> String {
    // Type 1/2 have to use the campus portal's GET protocol, which places
    // credentials in the query string. reqwest errors may otherwise include
    // that full URL and leak the password into app.log.
    error.without_url().to_string()
}

async fn fetch_portal_user_info(
    _local_ip: Option<&str>,
    compatibility: VpnCompatibility,
    route_context: Option<&PortalRouteContext>,
) -> Option<UserInfo> {
    // This is a credential-free, read-only current-session endpoint. Do not
    // suppress it merely because SSID/IP heuristics say "non-campus": users
    // may have a valid route through a VPN, and the dashboard should retain a
    // chance to refresh independently from the automatic-login trust policy.
    let client = portal_client(
        compatibility,
        &LoginType::Type3,
        std::time::Duration::from_secs(3),
        route_context,
    )
    .await
    .ok()?;
    let url = lgn_user_info_url(compatibility);
    let text = client.get(url).send().await.ok()?.text().await.ok()?;
    let start = text.find('(')?;
    let end = text.rfind(')')?;
    let data: serde_json::Value = serde_json::from_str(&text[start + 1..end]).ok()?;
    let info = data.get("user_info")?;
    let package_name = info
        .get("package_group_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let used_raw = info
        .get("use_flow")
        .and_then(|v| v.as_str())
        .unwrap_or("0GB");
    let remaining_flow = info
        .get("left_flow")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    let flow_pending = remaining_flow.is_none();
    let flow = remaining_flow.unwrap_or_else(|| "读取中…".to_string());
    Some(UserInfo {
        account: info
            .get("account")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        balance: info
            .get("balance")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        // Prefer the lightweight portal field. When it is absent, the signed
        // lgn2jfself handoff above reads the authoritative dashboard value;
        // never infer a quota from the package display name.
        flow,
        flow_pending,
        source: "portal".to_string(),
        status: None,
        status_reason: None,
        package: (!package_name.is_empty()).then(|| package_name.to_string()),
        package_detail: None,
        used_flow: Some(used_raw.to_string()),
        billing_cycle: None,
        updated_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        billing_error: None,
        login_history: Vec::new(),
        online_sessions: Vec::new(),
        offline_tip: None,
        mauth_enabled: None,
        billing_warnings: Vec::new(),
    })
}

#[tauri::command]
fn get_update_target(app: tauri::AppHandle) -> UpdateTarget {
    let platform = if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let raw_arch = std::env::consts::ARCH;
    let arch = if platform == "android" && raw_arch == "x86_64" {
        "x86_64"
    } else if raw_arch == "aarch64" {
        "arm64"
    } else {
        "x64"
    };
    let format = match platform {
        "android" => "apk",
        "ios" => "unsupported",
        "windows" => "exe",
        "macos" => "dmg",
        _ if std::env::var_os("APPIMAGE").is_some() => "AppImage",
        _ => "deb",
    };
    UpdateTarget {
        platform: platform.to_string(),
        arch: arch.to_string(),
        format: format.to_string(),
        current_version: app.package_info().version.to_string(),
    }
}

fn is_official_github_release_download(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url
            .path()
            .starts_with("/key-zhzr/BJUT-Auto-Login/releases/download/")
}

fn is_official_update_manifest_endpoint(url: &reqwest::Url) -> bool {
    let trusted_origin = url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.query().is_none()
        && url.fragment().is_none();
    if !trusted_origin || !url.path().ends_with("/latest.json") {
        return false;
    }
    url.path() == "/key-zhzr/BJUT-Auto-Login/releases/latest/download/latest.json"
        || url
            .path()
            .starts_with("/key-zhzr/BJUT-Auto-Login/releases/download/")
}

fn valid_update_manifest_version(version: &str) -> bool {
    let version = version.trim().strip_prefix('v').unwrap_or(version.trim());
    if version.is_empty() || version.len() > 64 {
        return false;
    }
    let (without_build, build) = version
        .split_once('+')
        .map_or((version, None), |(left, right)| (left, Some(right)));
    if build.is_some_and(|value| !valid_semver_identifiers(value, false)) {
        return false;
    }
    let (core, prerelease) = without_build
        .split_once('-')
        .map_or((without_build, None), |(left, right)| (left, Some(right)));
    if prerelease.is_some_and(|value| !valid_semver_identifiers(value, true)) {
        return false;
    }
    let mut core_parts = core.split('.');
    let valid_core = core_parts.by_ref().take(3).all(valid_semver_number);
    valid_core && core_parts.next().is_none() && core.split('.').count() == 3
}

fn valid_semver_number(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| character.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
}

fn valid_semver_identifiers(value: &str, reject_numeric_leading_zero: bool) -> bool {
    !value.is_empty()
        && value.split('.').all(|identifier| {
            !identifier.is_empty()
                && identifier
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
                && (!reject_numeric_leading_zero
                    || !identifier
                        .chars()
                        .all(|character| character.is_ascii_digit())
                    || valid_semver_number(identifier))
        })
}

fn latest_official_release_tag_from_atom(atom: &str) -> Option<String> {
    const RELEASE_TAG_LINK: &str =
        "href=\"https://github.com/key-zhzr/BJUT-Auto-Login/releases/tag/";
    atom.match_indices(RELEASE_TAG_LINK).find_map(|(index, _)| {
        let value = &atom[index + RELEASE_TAG_LINK.len()..];
        let tag = value.split_once('"')?.0;
        (tag.starts_with('v') && valid_update_manifest_version(tag)).then(|| tag.to_string())
    })
}

#[tauri::command]
async fn fetch_latest_official_release_tag() -> Result<String, String> {
    const RELEASES_ATOM_URL: &str = "https://github.com/key-zhzr/BJUT-Auto-Login/releases.atom";
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(5))
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(format!(
            "BJUT-Auto-Login/{} update-check",
            env!("CARGO_PKG_VERSION")
        ))
        .use_rustls_tls()
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(RELEASES_ATOM_URL)
        .header(
            reqwest::header::ACCEPT,
            "application/atom+xml, application/xml;q=0.9",
        )
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await
        .map_err(|error| format!("读取 GitHub 官方发布订阅失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub 官方发布订阅返回 HTTP {}",
            response.status()
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取 GitHub 官方发布订阅失败：{error}"))?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("GitHub 官方发布订阅大小异常".to_string());
    }
    let atom =
        std::str::from_utf8(&bytes).map_err(|_| "GitHub 官方发布订阅不是有效 UTF-8".to_string())?;
    latest_official_release_tag_from_atom(atom)
        .ok_or_else(|| "GitHub 官方发布订阅未包含有效版本".to_string())
}

#[tauri::command]
async fn get_release_asset_size(url: String) -> Result<Option<u64>, String> {
    update_metadata::asset_size(url).await
}

#[tauri::command]
async fn fetch_official_update_manifest(url: String) -> Result<serde_json::Value, String> {
    let endpoint = reqwest::Url::parse(&url).map_err(|error| error.to_string())?;
    if !is_official_update_manifest_endpoint(&endpoint) {
        return Err("拒绝读取非官方 GitHub 更新清单".to_string());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(5))
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(format!(
            "BJUT-Auto-Login/{} update-check",
            env!("CARGO_PKG_VERSION")
        ))
        .use_rustls_tls()
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(endpoint)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await
        .map_err(|error| format!("读取 GitHub 官方更新清单失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub 官方更新清单返回 HTTP {}",
            response.status()
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取 GitHub 官方更新清单失败：{error}"))?;
    if bytes.len() > 512 * 1024 {
        return Err("GitHub 官方更新清单大小异常".to_string());
    }
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("GitHub 官方更新清单格式异常：{error}"))?;
    let version = manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
        .filter(|version| valid_update_manifest_version(version))
        .ok_or_else(|| "GitHub 官方更新清单版本号无效".to_string())?;
    if manifest
        .get("platforms")
        .is_none_or(|value| !value.is_object())
    {
        return Err("GitHub 官方更新清单缺少平台签名信息".to_string());
    }
    let _ = version;
    Ok(manifest)
}

#[cfg(target_os = "android")]
fn launch_update_installer(_app: &tauri::AppHandle, path: &std::path::Path) -> Result<(), String> {
    use jni::objects::{JObject, JValue};

    let context = tauri::tao::platform::android::prelude::main_android_context()
        .ok_or_else(|| "Android context is unavailable".to_string())?;
    let vm = unsafe { jni::JavaVM::from_raw(context.java_vm.cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm
        .attach_current_thread_as_daemon()
        .map_err(|e| e.to_string())?;
    let activity = unsafe { JObject::from_raw(context.context_jobject.cast()) };
    let class =
        tauri::wry::prelude::find_class(&mut env, &activity, "cn.edu.bjut.al.UpdateHelper".into())
            .map_err(|e| e.to_string())?;
    let path_string = env
        .new_string(path.to_string_lossy().as_ref())
        .map_err(|e| e.to_string())?;
    let path_object = JObject::from(path_string);
    let launched = env
        .call_static_method(
            class,
            "installApk",
            "(Landroid/content/Context;Ljava/lang/String;)Z",
            &[JValue::Object(&activity), JValue::Object(&path_object)],
        )
        .map_err(|e| e.to_string())?
        .z()
        .map_err(|e| e.to_string())?;
    if launched {
        Ok(())
    } else {
        Err("无法启动 APK 安装器，请允许此应用安装未知来源应用后重试".to_string())
    }
}

#[cfg(target_os = "ios")]
fn launch_update_installer(_app: &tauri::AppHandle, _path: &std::path::Path) -> Result<(), String> {
    Err("iOS 版本不支持应用内安装，请使用快捷指令更新".to_string())
}

const UPDATE_DOWNLOAD_CANCELLED: &str = "更新包下载已停止";
const MAX_UPDATE_DOWNLOAD_BYTES: usize = 512 * 1024 * 1024;

async fn controlled_update_download(
    app: &tauri::AppHandle,
    control: &UpdateDownloadControl,
    url: reqwest::Url,
) -> Result<Vec<u8>, String> {
    use futures_util::StreamExt;

    if !is_official_github_release_download(&url) {
        return Err("拒绝下载非官方 GitHub Release 资产".to_string());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(8))
        .connect_timeout(std::time::Duration::from_secs(20))
        .use_rustls_tls()
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await
        .map_err(|error| format!("更新下载连接失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!("更新下载失败：HTTP {}", response.status()));
    }
    let total = response.content_length();
    if total.is_some_and(|size| size > MAX_UPDATE_DOWNLOAD_BYTES as u64) {
        return Err("更新包大小超过安全上限".to_string());
    }

    let mut received = 0u64;
    let mut bytes = Vec::with_capacity(
        total
            .unwrap_or_default()
            .min(MAX_UPDATE_DOWNLOAD_BYTES as u64) as usize,
    );
    let mut stream = response.bytes_stream();
    while let Some(chunk) = {
        while control.paused.load(Ordering::SeqCst) {
            if control.cancelled.load(Ordering::SeqCst) {
                let _ = app.emit(
                    "update-progress",
                    serde_json::json!({"status": "cancelled", "received": received, "total": total}),
                );
                return Err(UPDATE_DOWNLOAD_CANCELLED.to_string());
            }
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }
        if control.cancelled.load(Ordering::SeqCst) {
            let _ = app.emit(
                "update-progress",
                serde_json::json!({"status": "cancelled", "received": received, "total": total}),
            );
            return Err(UPDATE_DOWNLOAD_CANCELLED.to_string());
        }
        stream.next().await
    } {
        let chunk = chunk.map_err(|error| format!("读取更新包失败：{error}"))?;
        received = received.saturating_add(chunk.len() as u64);
        if received > MAX_UPDATE_DOWNLOAD_BYTES as u64 {
            return Err("更新包大小超过安全上限".to_string());
        }
        bytes.extend_from_slice(&chunk);
        let percent = total.map(|size| ((received as f64 / size as f64) * 100.0).min(100.0));
        let _ = app.emit(
            "update-progress",
            serde_json::json!({
                "status": "downloading",
                "received": received,
                "total": total,
                "percent": percent
            }),
        );
    }
    let _ = app.emit(
        "update-progress",
        serde_json::json!({
            "status": "verifying",
            "received": received,
            "total": total,
            "percent": 100.0
        }),
    );
    Ok(bytes)
}

#[cfg(desktop)]
fn verify_desktop_update_signature(bytes: &[u8], release_signature: &str) -> Result<(), String> {
    use base64::Engine;
    use minisign_verify::{PublicKey, Signature};

    const UPDATE_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDkxRkVDODJBOUJDRjYxNjgKUldSb1ljK2JLc2ora2FDUEl1L0tLNkF0alR3Rzl5UXF4U0JrNjhQZVdudGZmQjdmL1BHSVJoVysK";
    let decode_text = |value: &str| -> Result<String, String> {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(value)
            .map_err(|error| format!("更新签名 Base64 无效：{error}"))?;
        String::from_utf8(decoded).map_err(|_| "更新签名文本不是 UTF-8".to_string())
    };
    let public_key = PublicKey::decode(&decode_text(UPDATE_PUBLIC_KEY)?)
        .map_err(|error| format!("更新公钥无效：{error}"))?;
    let signature = Signature::decode(&decode_text(release_signature)?)
        .map_err(|error| format!("更新签名无效：{error}"))?;
    public_key
        .verify(bytes, &signature, true)
        .map_err(|error| format!("更新包签名验证失败：{error}"))
}

#[tauri::command]
fn control_update_download(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    action: String,
) -> Result<(), String> {
    if !state.update_download.active.load(Ordering::SeqCst) {
        return Err("当前没有正在进行的更新下载".to_string());
    }
    match action.as_str() {
        "pause" => {
            state.update_download.paused.store(true, Ordering::SeqCst);
            let _ = app.emit("update-progress", serde_json::json!({"status": "paused"}));
        }
        "resume" => {
            state.update_download.paused.store(false, Ordering::SeqCst);
            let _ = app.emit("update-progress", serde_json::json!({"status": "resumed"}));
        }
        "stop" => {
            state
                .update_download
                .cancelled
                .store(true, Ordering::SeqCst);
            state.update_download.paused.store(false, Ordering::SeqCst);
            let _ = app.emit("update-progress", serde_json::json!({"status": "stopping"}));
        }
        _ => return Err("不支持的更新下载控制操作".to_string()),
    }
    Ok(())
}

#[cfg(any(target_os = "android", target_os = "ios"))]
#[tauri::command]
async fn download_and_install_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    url: String,
    file_name: String,
) -> Result<(), String> {
    use std::io::Write;

    let parsed = reqwest::Url::parse(&url).map_err(|e| e.to_string())?;
    if !is_official_github_release_download(&parsed) {
        return Err("拒绝下载非官方 GitHub Release 资产".to_string());
    }
    let safe_name = std::path::Path::new(&file_name)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "无效的更新文件名".to_string())?;
    if safe_name != file_name {
        return Err("无效的更新文件名".to_string());
    }

    let mut update_dir = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    update_dir.push("updates");
    std::fs::create_dir_all(&update_dir).map_err(|e| e.to_string())?;
    let target_path = update_dir.join(safe_name);

    let _download_guard = state.update_download.begin()?;
    let bytes = controlled_update_download(&app, &state.update_download, parsed).await?;
    let mut file = std::fs::File::create(&target_path).map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    let _ = app.emit(
        "update-progress",
        serde_json::json!({"status": "installing", "percent": 100.0}),
    );
    launch_update_installer(&app, &target_path)
}

#[cfg(desktop)]
#[tauri::command]
async fn download_and_install_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    url: String,
    _file_name: String,
) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;

    let endpoint = reqwest::Url::parse(&url).map_err(|error| error.to_string())?;
    if !is_official_github_release_download(&endpoint) || !endpoint.path().ends_with("/latest.json")
    {
        return Err("拒绝使用非官方签名更新清单".to_string());
    }
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| error.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "签名更新清单未提供适用于当前设备的新版本".to_string())?;
    let _download_guard = state.update_download.begin()?;
    let bytes =
        controlled_update_download(&app, &state.update_download, update.download_url.clone())
            .await?;
    verify_desktop_update_signature(&bytes, &update.signature)?;
    let _ = app.emit(
        "update-progress",
        serde_json::json!({"status": "installing", "percent": 100.0}),
    );
    update
        .install(&bytes)
        .map_err(|error| format!("更新安装失败：{error}"))?;
    app.restart();
}

#[cfg(any(target_os = "android", target_os = "ios"))]
#[tauri::command]
async fn reinstall_current_version(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    url: String,
    file_name: String,
) -> Result<(), String> {
    download_and_install_update(app, state, url, file_name).await
}

#[cfg(desktop)]
#[tauri::command]
async fn reinstall_current_version(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    url: String,
    _file_name: String,
) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;

    let endpoint = reqwest::Url::parse(&url).map_err(|error| error.to_string())?;
    if !is_official_github_release_download(&endpoint) || !endpoint.path().ends_with("/latest.json")
    {
        return Err("拒绝使用非官方签名更新清单".to_string());
    }
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|error| error.to_string())?
        .version_comparator(|current, remote| remote.version == current)
        .build()
        .map_err(|error| error.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "签名更新清单不是当前应用版本，已拒绝重新安装".to_string())?;
    let _download_guard = state.update_download.begin()?;
    let bytes =
        controlled_update_download(&app, &state.update_download, update.download_url.clone())
            .await?;
    verify_desktop_update_signature(&bytes, &update.signature)?;
    let _ = app.emit(
        "update-progress",
        serde_json::json!({"status": "installing", "percent": 100.0}),
    );
    update
        .install(&bytes)
        .map_err(|error| format!("完整包安装失败：{error}"))?;
    app.restart();
}

fn show_native_notification(app: &tauri::AppHandle, title: &str, body: &str) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| e.to_string())
}

fn automatic_login_result_notifications_enabled(state: &AppState) -> bool {
    #[cfg(target_os = "android")]
    {
        state.config.read().unwrap().android_notify_login_results
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = state;
        true
    }
}

fn get_config_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let mut p = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap());
    let _ = std::fs::create_dir_all(&p);
    p.push("config.json");
    p
}

fn get_log_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let mut p = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap());
    let _ = std::fs::create_dir_all(&p);
    p.push("app.log");
    p
}

#[cfg(target_os = "macos")]
fn install_macos_panic_log_hook(app: &tauri::AppHandle) {
    let log_path = get_log_path(app);
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let message = panic_info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| {
                panic_info
                    .payload()
                    .downcast_ref::<String>()
                    .map(String::as_str)
            })
            .unwrap_or("unknown Rust panic")
            .replace(['\r', '\n'], " ");
        let location = panic_info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_else(|| "unknown location".to_string());
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            use std::io::Write;
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(
                file,
                "[{now}] [error] [系统] Rust panic at {location}: {}",
                message.chars().take(1000).collect::<String>()
            );
            let _ = file.flush();
        }
        previous_hook(panic_info);
    }));
}

fn get_account_health_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let mut path = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap());
    let _ = std::fs::create_dir_all(&path);
    path.push("account-health.json");
    path
}

fn load_account_health(app: &tauri::AppHandle) -> HashMap<String, AccountHealth> {
    std::fs::read_to_string(get_account_health_path(app))
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn persist_account_health(app: &tauri::AppHandle, health: &HashMap<String, AccountHealth>) {
    if let Ok(content) = serde_json::to_string_pretty(health) {
        let _ = std::fs::write(get_account_health_path(app), content);
    }
}

fn account_health_views(health: &HashMap<String, AccountHealth>) -> Vec<AccountHealthView> {
    let now = chrono::Utc::now().timestamp();
    let mut views: Vec<AccountHealthView> = health
        .iter()
        .map(|(user, item)| {
            let cooldown_seconds = item
                .cooldown_until
                .map(|until| (until - now).max(0))
                .unwrap_or(0);
            let status =
                if cooldown_seconds > 0 && item.failure_kind.as_deref() == Some("credential") {
                    "needs_attention"
                } else if cooldown_seconds > 0 {
                    "cooling_down"
                } else if item.consecutive_failures > 0 {
                    "degraded"
                } else {
                    "healthy"
                };
            AccountHealthView {
                user: user.clone(),
                status: status.to_string(),
                consecutive_failures: item.consecutive_failures,
                cooldown_until: item.cooldown_until,
                cooldown_seconds,
                last_success: item.last_success.clone(),
                last_failure: item.last_failure.clone(),
                last_failure_reason: item.last_failure_reason.clone(),
                failure_kind: item.failure_kind.clone(),
            }
        })
        .collect();
    views.sort_by(|a, b| a.user.cmp(&b.user));
    views
}

fn current_account_health_views(state: &AppState) -> Vec<AccountHealthView> {
    let mut health = state.account_health.lock().unwrap().clone();
    let users: Vec<String> = state
        .config
        .read()
        .unwrap()
        .accounts
        .iter()
        .map(|account| account.user.clone())
        .collect();
    for user in users {
        health.entry(user).or_default();
    }
    account_health_views(&health)
}

fn classify_account_failure(reason: &str, consecutive_failures: u32) -> (&'static str, i64) {
    let normalized = reason.to_ascii_lowercase();
    if reason.contains("密码")
        || reason.contains("账号不存在")
        || reason.contains("用户名")
        || normalized.contains("password")
        || normalized.contains("credential")
    {
        return ("credential", 30 * 60);
    }
    if reason.contains("余额") || reason.contains("欠费") || normalized.contains("balance") {
        return ("balance", 6 * 60 * 60);
    }
    let exponent = consecutive_failures.saturating_sub(1).min(6);
    if reason.contains("请求出错")
        || reason.contains("超时")
        || normalized.contains("timeout")
        || normalized.contains("connect")
    {
        return ("network", (15_i64 * (1_i64 << exponent)).min(15 * 60));
    }
    ("server", (60_i64 * (1_i64 << exponent)).min(15 * 60))
}

fn account_attempt_allowed(state: &AppState, user: &str) -> Result<(), i64> {
    let now = chrono::Utc::now().timestamp();
    let health = state.account_health.lock().unwrap();
    let remaining = health
        .get(user)
        .and_then(|item| item.cooldown_until)
        .map(|until| (until - now).max(0))
        .unwrap_or(0);
    if remaining > 0 {
        Err(remaining)
    } else {
        Ok(())
    }
}

fn emit_account_health(app: &tauri::AppHandle, state: &AppState) {
    let views = current_account_health_views(state);
    let _ = app.emit("account-health-change", views);
}

fn record_account_success(app: &tauri::AppHandle, state: &AppState, user: &str) {
    let snapshot = {
        let mut health = state.account_health.lock().unwrap();
        let item = health.entry(user.to_string()).or_default();
        item.consecutive_failures = 0;
        item.cooldown_until = None;
        item.last_success = Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
        item.last_failure_reason = None;
        item.failure_kind = None;
        health.clone()
    };
    persist_account_health(app, &snapshot);
    emit_account_health(app, state);
}

fn record_account_failure(app: &tauri::AppHandle, state: &AppState, user: &str, reason: &str) {
    let snapshot = {
        let mut health = state.account_health.lock().unwrap();
        let item = health.entry(user.to_string()).or_default();
        item.consecutive_failures = item.consecutive_failures.saturating_add(1);
        let (kind, cooldown_seconds) = classify_account_failure(reason, item.consecutive_failures);
        item.cooldown_until = Some(chrono::Utc::now().timestamp() + cooldown_seconds);
        item.last_failure = Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
        item.last_failure_reason = Some(reason.chars().take(200).collect());
        item.failure_kind = Some(kind.to_string());
        health.clone()
    };
    persist_account_health(app, &snapshot);
    emit_account_health(app, state);
}

fn reconcile_account_health_after_config_save(
    app: &tauri::AppHandle,
    state: &AppState,
    previous: &AppConfig,
    current: &AppConfig,
) {
    let snapshot = {
        let mut health = state.account_health.lock().unwrap();
        health.retain(|user, _| current.accounts.iter().any(|account| account.user == *user));
        for account in &current.accounts {
            let password_changed = previous
                .accounts
                .iter()
                .find(|candidate| candidate.user == account.user)
                .map(|candidate| candidate.pass != account.pass)
                .unwrap_or(true);
            if password_changed {
                health.remove(&account.user);
            }
        }
        health.clone()
    };
    persist_account_health(app, &snapshot);
    emit_account_health(app, state);
}

fn public_config(config: &AppConfig) -> AppConfig {
    let mut public = config.clone();
    for account in &mut public.accounts {
        account.pass.clear();
    }
    // Network trust rules and payment recovery metadata are stored only in the
    // encrypted credential backend, never in config.json.
    public.whitelist.clear();
    public.blacklist.clear();
    public.campus_service_sessions.clear();
    public.recharge_transactions = recharge_state::RechargeJournal::default();
    public
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn ensure_persistent_credential_backend() -> Result<(), String> {
    use keyring::credential::CredentialPersistence;

    match keyring::default::default_credential_builder().persistence() {
        CredentialPersistence::UntilDelete => Ok(()),
        _ => Err("系统凭据库后端不是持久存储，已拒绝读写以避免重启后丢失密码".to_string()),
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
const SECURE_CONFIG_SERVICE: &str = "cn.edu.bjut.al";
#[cfg(any(target_os = "windows", target_os = "linux"))]
const SECURE_CONFIG_USER: &str = "app-config";

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn load_secure_config(_app: &tauri::AppHandle) -> Result<Option<AppConfig>, String> {
    ensure_persistent_credential_backend()?;
    let entry = keyring::Entry::new(SECURE_CONFIG_SERVICE, SECURE_CONFIG_USER)
        .map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(serialized) => serde_json::from_str(&serialized)
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn save_secure_config(_app: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    ensure_persistent_credential_backend()?;
    let entry = keyring::Entry::new(SECURE_CONFIG_SERVICE, SECURE_CONFIG_USER)
        .map_err(|e| e.to_string())?;
    let serialized = serde_json::to_string(config).map_err(|e| e.to_string())?;
    entry.set_password(&serialized).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn macos_credential_paths(
    app: &tauri::AppHandle,
) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    use std::os::unix::fs::PermissionsExt;

    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    Ok((
        directory.join("credentials.key"),
        directory.join("credentials.enc"),
    ))
}

#[cfg(target_os = "macos")]
fn load_or_create_macos_credential_key(app: &tauri::AppHandle) -> Result<[u8; 32], String> {
    use aes_gcm::aead::{rand_core::RngCore, OsRng};
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let (key_path, _) = macos_credential_paths(app)?;
    if let Ok(bytes) = std::fs::read(&key_path) {
        return bytes
            .try_into()
            .map_err(|_| "macOS 本地凭据密钥长度无效".to_string());
    }
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&key_path)
        .map_err(|error| error.to_string())?;
    file.write_all(&key).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    Ok(key)
}

#[cfg(target_os = "macos")]
fn load_secure_config(app: &tauri::AppHandle) -> Result<Option<AppConfig>, String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};

    let (_, encrypted_path) = macos_credential_paths(app)?;
    let payload = match std::fs::read(encrypted_path) {
        Ok(payload) => payload,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if payload.len() < 13 || payload[0] != 1 {
        return Err("macOS 本地凭据文件格式无效".to_string());
    }
    let key = load_or_create_macos_credential_key(app)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|error| error.to_string())?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&payload[1..13]), &payload[13..])
        .map_err(|_| "macOS 本地凭据文件认证失败".to_string())?;
    serde_json::from_slice(&plaintext)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn save_secure_config(app: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    use aes_gcm::{
        aead::{rand_core::RngCore, Aead, OsRng},
        Aes256Gcm, KeyInit, Nonce,
    };
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let key = load_or_create_macos_credential_key(app)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|error| error.to_string())?;
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let serialized = serde_json::to_vec(config).map_err(|error| error.to_string())?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), serialized.as_ref())
        .map_err(|_| "macOS 本地凭据加密失败".to_string())?;
    let (_, encrypted_path) = macos_credential_paths(app)?;
    let temporary_path = encrypted_path.with_extension("enc.tmp");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary_path)
        .map_err(|error| error.to_string())?;
    file.write_all(&[1])
        .and_then(|_| file.write_all(&nonce))
        .and_then(|_| file.write_all(&ciphertext))
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(temporary_path, encrypted_path).map_err(|error| error.to_string())
}

fn save_secure_config_verified(app: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    save_secure_config(app, config)?;
    match load_secure_config(app)? {
        Some(persisted) if persisted == *config => Ok(()),
        Some(_) => Err("安全存储回读内容与待保存配置不一致".to_string()),
        None => Err("安全存储写入后未能回读配置".to_string()),
    }
}

const CONFIG_BACKUP_VERSION: u8 = 3;
const CONFIG_BACKUP_ITERATIONS: u32 = 250_000;
const CONFIG_BACKUP_MAX_BYTES: usize = 2 * 1024 * 1024;

fn validate_config_backup_passphrase(passphrase: &[u8]) -> Result<(), String> {
    if passphrase.len() < 8 {
        return Err("配置备份密码至少需要 8 个字节".to_string());
    }
    if passphrase.len() > 1024 {
        return Err("配置备份密码过长".to_string());
    }
    Ok(())
}

fn config_backup_key(passphrase: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(passphrase, salt, iterations, &mut key);
    key
}

fn encrypt_config_backup_payload(
    plaintext: &ConfigBackupPlaintext,
    passphrase: &[u8],
) -> Result<String, String> {
    use aes_gcm::{
        aead::{rand_core::RngCore, Aead, OsRng},
        Aes256Gcm, KeyInit, Nonce,
    };
    use base64::Engine;

    validate_config_backup_passphrase(passphrase)?;
    let mut salt = [0u8; 16];
    let mut iv = [0u8; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut iv);
    let mut key = config_backup_key(passphrase, &salt, CONFIG_BACKUP_ITERATIONS);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|error| error.to_string())?;
    let mut serialized = serde_json::to_vec(plaintext).map_err(|error| error.to_string())?;
    if serialized.len() > CONFIG_BACKUP_MAX_BYTES {
        serialized.fill(0);
        key.fill(0);
        return Err("配置备份内容超过安全大小限制".to_string());
    }
    let encrypted = cipher
        .encrypt(Nonce::from_slice(&iv), serialized.as_ref())
        .map_err(|_| "配置备份加密失败".to_string());
    serialized.fill(0);
    key.fill(0);
    let encrypted = encrypted?;
    let base64 = base64::engine::general_purpose::STANDARD;
    serde_json::to_string(&EncryptedConfigBackup {
        version: CONFIG_BACKUP_VERSION,
        kdf: "PBKDF2-HMAC-SHA256".to_string(),
        iterations: CONFIG_BACKUP_ITERATIONS,
        salt: base64.encode(salt),
        iv: base64.encode(iv),
        ciphertext: base64.encode(encrypted),
    })
    .map_err(|error| error.to_string())
}

fn decrypt_config_backup_payload(
    payload: &str,
    passphrase: &[u8],
) -> Result<ConfigBackupPlaintext, String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
    use base64::Engine;

    validate_config_backup_passphrase(passphrase)?;
    if payload.len() > CONFIG_BACKUP_MAX_BYTES * 2 {
        return Err("配置备份超过安全大小限制".to_string());
    }
    let envelope: EncryptedConfigBackup =
        serde_json::from_str(payload).map_err(|_| "配置备份外层格式无效".to_string())?;
    if envelope.version != CONFIG_BACKUP_VERSION
        || envelope.kdf != "PBKDF2-HMAC-SHA256"
        || !(100_000..=1_000_000).contains(&envelope.iterations)
    {
        return Err("不是受支持的完整配置备份格式".to_string());
    }
    let base64 = base64::engine::general_purpose::STANDARD;
    let salt = base64
        .decode(envelope.salt)
        .map_err(|_| "配置备份盐值无效".to_string())?;
    let iv = base64
        .decode(envelope.iv)
        .map_err(|_| "配置备份随机向量无效".to_string())?;
    let ciphertext = base64
        .decode(envelope.ciphertext)
        .map_err(|_| "配置备份密文无效".to_string())?;
    if salt.len() != 16 || iv.len() != 12 || ciphertext.len() > CONFIG_BACKUP_MAX_BYTES {
        return Err("配置备份参数长度无效".to_string());
    }
    let mut key = config_backup_key(passphrase, &salt, envelope.iterations);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|error| error.to_string())?;
    let decrypted = cipher
        .decrypt(Nonce::from_slice(&iv), ciphertext.as_ref())
        .map_err(|_| "配置备份密码错误或内容已损坏".to_string());
    key.fill(0);
    let mut decrypted = decrypted?;
    let plaintext = serde_json::from_slice::<ConfigBackupPlaintext>(&decrypted)
        .map_err(|_| "配置备份内容无法解析".to_string());
    decrypted.fill(0);
    let plaintext = plaintext?;
    if plaintext.format != "BJUT-AL-CONFIG" || plaintext.version != CONFIG_BACKUP_VERSION {
        return Err("配置备份内容版本不匹配".to_string());
    }
    Ok(plaintext)
}

fn validate_imported_config(config: &AppConfig) -> Result<(), String> {
    use std::collections::HashSet;

    if config.accounts.len() > 100 {
        return Err("配置备份中的账号数量超过 100 个".to_string());
    }
    if config.network_profiles.len() > 200 {
        return Err("配置备份中的网络档案数量超过 200 个".to_string());
    }
    let mut users = HashSet::new();
    for account in &config.accounts {
        let user = account.user.trim();
        if user.is_empty() || user.len() > 128 {
            return Err("配置备份包含空账号或异常长度账号".to_string());
        }
        if account.pass.len() > 1024 {
            return Err(format!("账号 {user} 的密码长度异常"));
        }
        if !users.insert(user.to_string()) {
            return Err(format!("配置备份包含重复账号 {user}"));
        }
    }
    Ok(())
}

fn config_backup_counts(config: &AppConfig) -> (usize, usize, Vec<String>) {
    let missing = config
        .accounts
        .iter()
        .filter(|account| account.pass.is_empty())
        .map(|account| account.user.clone())
        .collect::<Vec<_>>();
    (
        config.accounts.len(),
        config.accounts.len().saturating_sub(missing.len()),
        missing,
    )
}

#[cfg(target_os = "android")]
fn android_secure_config(value: Option<&str>) -> Result<Option<String>, String> {
    use jni::objects::{JObject, JString, JValue};

    let context = tauri::tao::platform::android::prelude::main_android_context()
        .ok_or_else(|| "Android context is unavailable".to_string())?;
    let vm = unsafe { jni::JavaVM::from_raw(context.java_vm.cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm
        .attach_current_thread_as_daemon()
        .map_err(|e| e.to_string())?;
    let activity = unsafe { JObject::from_raw(context.context_jobject.cast()) };
    let class =
        tauri::wry::prelude::find_class(&mut env, &activity, "cn.edu.bjut.al.NetworkHelper".into())
            .map_err(|e| e.to_string())?;

    if let Some(value) = value {
        let value = env.new_string(value).map_err(|e| e.to_string())?;
        let value_object = JObject::from(value);
        let saved = env.call_static_method(
            class,
            "setSecureConfig",
            "(Landroid/content/Context;Ljava/lang/String;)Z",
            &[JValue::Object(&activity), JValue::Object(&value_object)],
        );
        let saved = match saved {
            Ok(value) => value.z().map_err(|e| e.to_string())?,
            Err(error) => {
                let _ = env.exception_clear();
                return Err(format!("Android secure storage write failed: {error}"));
            }
        };
        return if saved {
            Ok(None)
        } else {
            Err("Android Keystore refused the configuration".to_string())
        };
    }

    let result = env.call_static_method(
        class,
        "getSecureConfig",
        "(Landroid/content/Context;)Ljava/lang/String;",
        &[JValue::Object(&activity)],
    );
    let result = match result {
        Ok(value) => value.l().map_err(|e| e.to_string())?,
        Err(error) => {
            let _ = env.exception_clear();
            return Err(format!("Android secure storage read failed: {error}"));
        }
    };
    if result.is_null() {
        return Ok(None);
    }
    let value_string = JString::from(result);
    let value = env.get_string(&value_string).map_err(|e| e.to_string())?;
    let value: String = value.into();
    Ok((!value.is_empty()).then_some(value))
}

#[cfg(target_os = "android")]
fn load_secure_config(_app: &tauri::AppHandle) -> Result<Option<AppConfig>, String> {
    android_secure_config(None)?
        .map(|serialized| serde_json::from_str(&serialized).map_err(|e| e.to_string()))
        .transpose()
}

#[cfg(target_os = "android")]
fn save_secure_config(_app: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    let serialized = serde_json::to_string(config).map_err(|e| e.to_string())?;
    android_secure_config(Some(&serialized)).map(|_| ())
}

fn write_public_config(app: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    let content =
        serde_json::to_string_pretty(&public_config(config)).map_err(|e| e.to_string())?;
    std::fs::write(get_config_path(app), content).map_err(|e| e.to_string())
}

const LOG_SESSION_MARKER: &str = "=== SESSION START ===";
const MAX_LOG_SESSIONS: usize = 5;
const MAX_LOG_ENTRIES: usize = 5000;
#[cfg(target_os = "android")]
const ANDROID_KEEPALIVE_JOURNAL: &str = "keepalive-journal.log";

fn parse_log_line(line: &str) -> Option<LogEntry> {
    let idx1 = line.find(']')?;
    line.strip_prefix('[')?;
    let time = line[1..idx1].to_string();
    let rest = &line[idx1 + 1..];
    let idx2 = rest.find('[')?;
    let idx3 = rest[idx2..].find(']')? + idx2;
    let log_type = rest[idx2 + 1..idx3].to_string();
    let rest2 = &rest[idx3 + 1..];
    let idx4 = rest2.find('[')?;
    let idx5 = rest2[idx4..].find(']')? + idx4;
    Some(LogEntry {
        time,
        module: rest2[idx4 + 1..idx5].to_string(),
        message: rest2[idx5 + 1..].trim().to_string(),
        log_type,
    })
}

fn initialize_log_history(app: &tauri::AppHandle, state: &AppState) {
    let path = get_log_path(app);
    #[allow(unused_mut)]
    let mut existing = std::fs::read_to_string(&path).unwrap_or_default();
    #[cfg(target_os = "android")]
    let mut imported_journals = Vec::new();
    #[cfg(target_os = "android")]
    if let Some(parent) = path.parent() {
        let journal_path = parent.join(ANDROID_KEEPALIVE_JOURNAL);
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let importing_path = parent.join(format!("keepalive-journal.importing-{suffix}.log"));
        if std::fs::rename(&journal_path, &importing_path).is_ok() {
            imported_journals.push(importing_path);
        }
        // Also recover an import left behind if the previous startup stopped
        // between the atomic rename and the final app.log write.
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let file_name = file_name.to_string_lossy();
                if file_name.starts_with("keepalive-journal.importing")
                    && file_name.ends_with(".log")
                    && !imported_journals.contains(&entry.path())
                {
                    imported_journals.push(entry.path());
                }
            }
        }
        imported_journals.sort();
        imported_journals.retain(|importing_path| {
            if let Ok(journal) = std::fs::read_to_string(importing_path) {
                if !existing.is_empty() && !existing.ends_with('\n') {
                    existing.push('\n');
                }
                existing.push_str(&journal);
                true
            } else {
                false
            }
        });
    }
    let existing_lines: Vec<&str> = existing.lines().collect();
    let session_starts: Vec<usize> = existing_lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| line.contains(LOG_SESSION_MARKER).then_some(index))
        .collect();
    let keep_from = if session_starts.len() >= MAX_LOG_SESSIONS {
        session_starts[session_starts.len() - (MAX_LOG_SESSIONS - 1)]
    } else {
        0
    };
    let mut lines: Vec<String> = existing_lines[keep_from..]
        .iter()
        .map(|line| (*line).to_string())
        .collect();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    lines.push(format!(
        "[{}] [info] [系统] {} {}",
        now, LOG_SESSION_MARKER, now
    ));
    let serialized = format!("{}\n", lines.join("\n"));
    let history_written = std::fs::write(path, serialized).is_ok();
    #[cfg(target_os = "android")]
    if history_written {
        for importing_path in imported_journals {
            let _ = std::fs::remove_file(importing_path);
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = history_written;

    let mut memory = state.logs.lock().unwrap();
    memory.clear();
    memory.extend(lines.iter().filter_map(|line| parse_log_line(line)));
    if memory.len() > MAX_LOG_ENTRIES {
        let remove_count = memory.len() - MAX_LOG_ENTRIES;
        memory.drain(..remove_count);
    }
}

fn rust_log(app: &tauri::AppHandle, state: &AppState, module: &str, message: &str, log_type: &str) {
    network_events::from_log(app, state, module, message, log_type);
    let current_level = {
        let cfg = state.config.read().unwrap();
        cfg.log_level.clone()
    };
    let hidden = (current_level == "error" && log_type != "error")
        || (current_level == "info" && log_type == "debug");
    if hidden && module != "登录反馈" {
        return;
    }
    let local_now = chrono::Local::now();
    let time_str = local_now.format("%Y-%m-%d %H:%M:%S").to_string();
    let entry = LogEntry {
        time: time_str,
        module: module.to_string(),
        message: message.to_string(),
        log_type: log_type.to_string(),
    };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(get_log_path(app))
    {
        use std::io::Write;
        let log_line = format!(
            "[{}] [{}] [{}] {}\n",
            entry.time, entry.log_type, entry.module, entry.message
        );
        let _ = file.write_all(log_line.as_bytes());
    }
    if hidden {
        return;
    }
    {
        let mut logs = state.logs.lock().unwrap();
        logs.push(entry.clone());
        if logs.len() > MAX_LOG_ENTRIES {
            logs.remove(0);
        }
    }
    let _ = app.emit("log-event", entry);
}

fn merge_legacy_credentials(current: &mut AppConfig, legacy: &AppConfig) -> bool {
    let mut changed = false;
    for legacy_account in &legacy.accounts {
        if let Some(index) = current
            .accounts
            .iter()
            .position(|account| account.user == legacy_account.user)
        {
            let account = &mut current.accounts[index];
            if account.pass.is_empty() && !legacy_account.pass.is_empty() {
                account.pass = legacy_account.pass.clone();
                changed = true;
            }
        } else {
            let mut appended = legacy_account.clone();
            if current.accounts.iter().any(|account| account.is_default) {
                appended.is_default = false;
            }
            current.accounts.push(appended);
            changed = true;
        }
    }

    if !current.accounts.is_empty() && !current.accounts.iter().any(|account| account.is_default) {
        current.accounts[0].is_default = true;
        changed = true;
    }
    changed
}

fn fill_missing_passwords(target: &mut AppConfig, existing: &AppConfig) -> bool {
    let mut changed = false;
    for account in &mut target.accounts {
        if !account.pass.is_empty() {
            continue;
        }
        if let Some(saved) = existing
            .accounts
            .iter()
            .find(|candidate| candidate.user == account.user && !candidate.pass.is_empty())
        {
            account.pass = saved.pass.clone();
            changed = true;
        }
    }
    changed
}

fn load_config(app: &tauri::AppHandle, state: &AppState) {
    let p = get_config_path(app);
    let disk_config = std::fs::read_to_string(p)
        .ok()
        .and_then(|content| serde_json::from_str::<AppConfig>(&content).ok());

    match load_secure_config(app) {
        Ok(Some(mut config)) => {
            let migrated = disk_config
                .as_ref()
                .map(|legacy| merge_legacy_credentials(&mut config, legacy))
                .unwrap_or(false);
            let migration_persisted = if migrated {
                if let Err(error) = save_secure_config_verified(app, &config) {
                    eprintln!("Unable to migrate legacy credentials into secure storage: {error}");
                    false
                } else {
                    true
                }
            } else {
                true
            };
            // Do not erase the legacy plaintext passwords until the secure copy
            // has definitely been written; it may be the only recoverable copy.
            if migration_persisted && !migrated {
                let _ = write_public_config(app, &config);
            }
            *state.credential_storage_status.lock().unwrap() = if migration_persisted {
                "available".to_string()
            } else {
                "error".to_string()
            };
            *state.config.write().unwrap() = config;
        }
        Ok(None) => {
            if let Some(config) = disk_config {
                // Migrate legacy plaintext configurations once the system credential store is available.
                let mut status = "missing";
                if config
                    .accounts
                    .iter()
                    .any(|account| !account.pass.is_empty())
                {
                    if let Err(error) = save_secure_config_verified(app, &config) {
                        eprintln!("Unable to migrate credentials to secure storage: {error}");
                        status = "error";
                    } else {
                        // Keep the legacy file for one complete restart. The
                        // Ok(Some) branch removes its passwords only after the
                        // system credential store returns the same data later.
                        status = "available";
                    }
                }
                *state.credential_storage_status.lock().unwrap() = status.to_string();
                *state.config.write().unwrap() = config;
            } else {
                *state.credential_storage_status.lock().unwrap() = "missing".to_string();
            }
        }
        Err(error) => {
            eprintln!("Unable to read secure credentials: {error}");
            *state.credential_storage_status.lock().unwrap() = "error".to_string();
            // Keep old installations usable even when the system credential store
            // is temporarily unavailable. A later explicit save still has to pass.
            if let Some(config) = disk_config {
                *state.config.write().unwrap() = config;
            }
        }
    }
}

fn save_config(
    app: &tauri::AppHandle,
    state: &AppState,
    mut new_cfg: AppConfig,
) -> Result<(), String> {
    if new_cfg.android_notification_mode != "separate" {
        new_cfg.android_notification_mode = default_android_notification_mode();
    }
    if new_cfg.theme == "apple26" {
        new_cfg.theme = "apple27".to_string();
    } else if !matches!(new_cfg.theme.as_str(), "basic" | "apple27" | "winui") {
        new_cfg.theme = default_theme();
    }
    if !matches!(
        new_cfg.accent_color.as_str(),
        "blue" | "violet" | "cyan" | "green" | "orange" | "rose"
    ) {
        new_cfg.accent_color = default_accent_color();
    }
    if !matches!(new_cfg.color_mode.as_str(), "system" | "dark" | "light") {
        new_cfg.color_mode = default_color_mode();
    }
    normalize_trust_lists(&mut new_cfg.whitelist, &mut new_cfg.blacklist);
    let previous_cfg = {
        let state_cfg = state.config.read().unwrap();
        fill_missing_passwords(&mut new_cfg, &state_cfg);
        state_cfg.clone()
    };
    let billing_credentials_changed = previous_cfg.accounts.len() != new_cfg.accounts.len()
        || previous_cfg.accounts.iter().any(|before| {
            new_cfg
                .accounts
                .iter()
                .find(|after| after.user == before.user)
                .is_none_or(|after| after.pass != before.pass)
        });
    new_cfg.campus_service_sessions = previous_cfg
        .campus_service_sessions
        .iter()
        .filter(|session| {
            previous_cfg
                .accounts
                .iter()
                .find(|account| account.user == session.account())
                .zip(
                    new_cfg
                        .accounts
                        .iter()
                        .find(|account| account.user == session.account()),
                )
                .is_some_and(|(before, after)| before.pass == after.pass)
        })
        .cloned()
        .collect();
    // Recovery records are backend-owned. The WebView intentionally never
    // receives them, so ordinary setting/account saves must not erase an
    // unfinished payment.
    new_cfg.recharge_transactions = previous_cfg.recharge_transactions.clone();
    let storage_was_unreadable = {
        let status = state.credential_storage_status.lock().unwrap();
        status.as_str() == "error" || status.as_str() == "unknown"
    };
    if storage_was_unreadable
        && new_cfg
            .accounts
            .iter()
            .any(|account| account.pass.is_empty())
    {
        match load_secure_config(app) {
            Ok(Some(saved)) => {
                fill_missing_passwords(&mut new_cfg, &saved);
            }
            Ok(None) => {}
            Err(error) => {
                return Err(format!(
                    "安全存储暂时不可读，已阻止空密码覆盖原数据: {error}"
                ));
            }
        }
    }
    save_secure_config_verified(app, &new_cfg)?;
    *state.credential_storage_status.lock().unwrap() = "available".to_string();
    write_public_config(app, &new_cfg)?;
    {
        let mut state_cfg = state.config.write().unwrap();
        *state_cfg = new_cfg.clone();
    }
    if billing_credentials_changed {
        state.billing_sessions.lock().unwrap().clear();
    }
    reconcile_account_health_after_config_save(app, state, &previous_cfg, &new_cfg);
    #[cfg(desktop)]
    refresh_tray_menu(app, state);
    Ok(())
}

struct NetworkScheduleGuard {
    app: tauri::AppHandle,
    state: Arc<AppState>,
    login_generation: u64,
    network_generation: u64,
}

impl Drop for NetworkScheduleGuard {
    fn drop(&mut self) {
        if network_check_login_operation_superseded(&self.state, self.login_generation)
            || self.state.network_change_generation.load(Ordering::SeqCst)
                != self.network_generation
        {
            return;
        }
        let network = self.state.last_network_state.lock().unwrap().clone();
        let cfg = self.state.config.read().unwrap().clone();
        let background = self.state.is_in_background.load(Ordering::SeqCst);
        let profile = matching_network_profile(
            &cfg,
            network["ssid"].as_str().unwrap_or(""),
            network["bssid"].as_str().unwrap_or(""),
            &login_type_from_profile(network["loginType"].as_str().unwrap_or(""))
                .unwrap_or(LoginType::Unknown),
        );
        let configured = profile
            .as_ref()
            .and_then(|profile| {
                if background {
                    profile.check_interval_bg
                } else {
                    profile.check_interval
                }
            })
            .unwrap_or(if background {
                cfg.check_interval_bg
            } else {
                cfg.check_interval
            });
        let identity = format!(
            "{}|{}",
            network["interfaceName"].as_str().unwrap_or(""),
            network["ip"].as_str().unwrap_or("")
        );
        let plan =
            self.state
                .network_schedule
                .lock()
                .unwrap()
                .observe(network_schedule::ScheduleInput {
                    identity: &identity,
                    online: network["state"] == "Online",
                    has_link: !network["ip"].as_str().unwrap_or("").is_empty(),
                    background,
                    mobile_data: is_mobile_data_network(&network),
                    power_saving: network_schedule::power_saving(),
                    enabled: cfg.adaptive_network_checks,
                    configured,
                });
        self.state
            .countdown
            .store(plan.interval_seconds, Ordering::SeqCst);
        let _ = self.app.emit("network-schedule", &plan);
    }
}

#[tauri::command]
fn get_network_schedule(state: tauri::State<Arc<AppState>>) -> network_schedule::SchedulePlan {
    state.network_schedule.lock().unwrap().plan.clone()
}

#[tauri::command]
fn get_network_check_progress(
    state: tauri::State<Arc<AppState>>,
) -> Option<network_progress::Progress> {
    state.network_progress.snapshot()
}

async fn trigger_network_check(app: tauri::AppHandle, state: Arc<AppState>, full_details: bool) {
    if state.is_checking.swap(true, Ordering::SeqCst) {
        if full_details {
            state.pending_full_check.store(true, Ordering::SeqCst);
        }
        return;
    }
    if full_details {
        state.pending_full_check.store(false, Ordering::SeqCst);
    }
    // A manual login/account switch advances this generation. Any already
    // running connectivity check may finish its read-only probes, but it must
    // not subsequently submit credentials or overwrite the newer result.
    let login_operation_generation = state.login_operation_generation.load(Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        if state.manual_login_in_progress.load(Ordering::SeqCst) {
            rust_log(
                &app,
                &state,
                "网络",
                "手动登录或切换账号正在进行，已跳过本轮倒计时检测",
                "debug",
            );
            state.is_checking.store(false, Ordering::SeqCst);
            return;
        }
        let _schedule_guard = NetworkScheduleGuard {
            app: app.clone(),
            state: state.clone(),
            login_generation: login_operation_generation,
            network_generation: state.network_change_generation.load(Ordering::SeqCst),
        };
        let progress = network_progress::Run::begin(
            &app,
            &state,
            _schedule_guard.network_generation,
            "正在核对认证网卡与 IP 地址",
        );
        let is_bg = app_is_in_background(&app, &state);
        state.is_in_background.store(is_bg, Ordering::SeqCst);
        let (interval_fg, interval_bg, compatibility) = {
            let cfg = state.config.read().unwrap();
            (
                cfg.check_interval,
                cfg.check_interval_bg,
                effective_vpn_compatibility(&cfg),
            )
        };
        let _ = app.emit("countdown-tick", serde_json::json!({"status": "checking"}));
        rust_log(
            &app,
            &state,
            "网络",
            &format!(
                "[DEBUG] 开始检测网络连通性 (模式: {})",
                if is_bg { "后台" } else { "前台" }
            ),
            "debug",
        );

        // Connectivity-only checks reuse the last details and avoid location-protected APIs.
        #[cfg(target_os = "macos")]
        if is_bg && !full_details {
            rust_log(
                &app,
                &state,
                "隐私",
                "macOS 后台普通检测已跳过 SSID/BSSID，仅检查网络连通性",
                "debug",
            );
        }
        #[cfg(target_os = "macos")]
        if is_bg && full_details {
            rust_log(
                &app,
                &state,
                "隐私",
                "macOS 后台完整检测已触发，将重新核对当前物理接口",
                "debug",
            );
        }
        #[cfg(target_os = "android")]
        let net_info = get_network_info(app.clone(), Some(full_details));
        #[cfg(not(target_os = "android"))]
        let net_info = get_network_info(app.clone(), Some(full_details));
        let previous_network = state.last_network_state.lock().unwrap().clone();
        #[cfg(target_os = "android")]
        let portal_route_context = portal_route_context_from_network(&net_info).ok().flatten();
        #[cfg(not(target_os = "android"))]
        let mut portal_route_context = portal_route_context_from_network(&net_info).ok().flatten();
        #[cfg(not(target_os = "android"))]
        let mut trusted_portal_identity = NetworkIdentitySnapshot::capture(&net_info);
        #[cfg(target_os = "android")]
        let wifi_route_guard = AndroidWifiRouteGuard::bind_if_required(&net_info);
        let transport = network_transport(&net_info).to_string();
        let is_mobile_data = is_mobile_data_network(&net_info);
        let was_mobile_data = is_mobile_data_network(&previous_network);
        #[cfg(target_os = "android")]
        let system_validated = net_info
            .get("validated")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        #[cfg(target_os = "android")]
        let metered = net_info
            .get("metered")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let raw_ssid = net_info.get("ssid").and_then(|v| v.as_str()).unwrap_or("");
        let raw_bssid = net_info.get("bssid").and_then(|v| v.as_str()).unwrap_or("");
        let raw_ip = net_info.get("ip").and_then(|v| v.as_str()).unwrap_or("");
        #[cfg(target_os = "android")]
        let preserve_wifi_identity = false;
        #[cfg(not(target_os = "android"))]
        let preserve_wifi_identity = !full_details && transport.eq_ignore_ascii_case("wifi");
        let initial_ssid = if preserve_wifi_identity && raw_ssid.is_empty() {
            previous_network
                .get("ssid")
                .and_then(|value| value.as_str())
                .unwrap_or("")
        } else {
            raw_ssid
        }
        .to_string();
        let mut current_ssid = initial_ssid;

        let initial_bssid = if preserve_wifi_identity && raw_bssid.is_empty() {
            previous_network
                .get("bssid")
                .and_then(|value| value.as_str())
                .unwrap_or("")
        } else {
            raw_bssid
        }
        .to_string();
        let mut current_bssid = initial_bssid;

        let initial_ip = if preserve_wifi_identity && raw_ip.is_empty() {
            previous_network
                .get("ip")
                .and_then(|value| value.as_str())
                .unwrap_or("")
        } else {
            raw_ip
        }
        .to_string();
        let mut current_ip = initial_ip;
        #[cfg(target_os = "android")]
        let mut identity_fresh = net_info
            .get("identityFresh")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        #[cfg(not(target_os = "android"))]
        let mut identity_fresh = network_identity_is_fresh(&net_info);
        let preliminary_profile = {
            let cfg = state.config.read().unwrap();
            matching_network_profile(&cfg, &current_ssid, &current_bssid, &LoginType::Unknown)
        };
        let mut next_interval = if is_bg { interval_bg } else { interval_fg };
        if let Some(profile) = preliminary_profile.as_ref() {
            let interval = if is_bg {
                profile.check_interval_bg
            } else {
                profile.check_interval
            };
            if let Some(interval) = interval.filter(|value| *value >= 5) {
                next_interval = interval;
            }
        }
        if is_mobile_data {
            next_interval = mobile_data_check_interval(next_interval, is_bg);
            rust_log(
                &app,
                &state,
                "网络",
                &format!(
                    "{}移动数据网络，自动检测间隔为 {} 秒，校园网认证网关探测已停用",
                    if was_mobile_data {
                        "继续使用"
                    } else {
                        "检测到"
                    },
                    next_interval
                ),
                if was_mobile_data { "debug" } else { "info" },
            );
        }
        state.countdown.store(next_interval, Ordering::SeqCst);
        let local_now = chrono::Local::now();
        let timestamp = local_now.format("%Y-%m-%d %H:%M:%S").to_string();

        if full_details {
            #[cfg(target_os = "android")]
            rust_log(
                &app,
                &state,
                "网络",
                &format!(
                    "[DEBUG] 完整检测网络详情: SSID={}, BSSID={}, IP={}, 传输类型={}, 系统验证={}",
                    current_ssid, current_bssid, current_ip, transport, system_validated
                ),
                "debug",
            );
            #[cfg(not(target_os = "android"))]
            rust_log(
                &app,
                &state,
                "网络",
                &format!(
                    "[DEBUG] 完整检测网络详情: SSID={}, BSSID={}, IP={}",
                    current_ssid, current_bssid, current_ip
                ),
                "debug",
            );
        } else {
            #[cfg(target_os = "android")]
            rust_log(
                &app,
                &state,
                "网络",
                &format!("[DEBUG] 后台间隔检测仅检查连通性，传输类型={}", transport),
                "debug",
            );
            #[cfg(not(target_os = "android"))]
            rust_log(
                &app,
                &state,
                "网络",
                "[DEBUG] 后台间隔检测仅检查连通性，复用上次网络详情",
                "debug",
            );
        }

        let make_payload =
            |state_str: &str, login_type: Option<&LoginType>, ssid: &str, bssid: &str, ip: &str| {
                let mut payload = net_info.clone();
                if !payload.is_object() {
                    payload = serde_json::json!({});
                }
                if let Some(object) = payload.as_object_mut() {
                    object.insert("state".to_string(), serde_json::json!(state_str));
                    object.insert(
                        "systemOnline".to_string(),
                        serde_json::json!(state.system_online.load(Ordering::SeqCst)),
                    );
                    object.insert(
                        "loginType".to_string(),
                        serde_json::json!(login_type.map(LoginType::as_str)),
                    );
                    object.insert("ssid".to_string(), serde_json::json!(ssid));
                    object.insert("bssid".to_string(), serde_json::json!(bssid));
                    object.insert("ip".to_string(), serde_json::json!(ip));
                    object.insert(
                        "timestamp".to_string(),
                        serde_json::json!(timestamp.clone()),
                    );
                    object.insert(
                        "transport".to_string(),
                        serde_json::json!(transport.clone()),
                    );
                    #[cfg(target_os = "android")]
                    object.insert("validated".to_string(), serde_json::json!(system_validated));
                    #[cfg(target_os = "android")]
                    object.insert("metered".to_string(), serde_json::json!(metered));
                }
                payload
            };

        #[cfg(target_os = "android")]
        if wifi_route_guard.failed() {
            let message =
                "检测到非默认的待认证 Wi-Fi，但无法将请求绑定到该 Wi-Fi；已停止使用 VPN/移动数据结果";
            rust_log(&app, &state, "网络", message, "error");
            state.is_checking.store(false, Ordering::SeqCst);
            state.non_campus_count.store(0, Ordering::SeqCst);
            let mut payload = make_payload(
                "BjutCampus",
                None,
                &current_ssid,
                &current_bssid,
                &current_ip,
            );
            if let Some(object) = payload.as_object_mut() {
                object.insert("loginMessage".to_string(), serde_json::json!(message));
            }
            *state.last_network_state.lock().unwrap() = payload.clone();
            let _ = app.emit("network-state-change", payload);
            return;
        }

        // Android's VALIDATED bit is useful context, but it can describe a
        // different default network (for example cellular while an unvalidated
        // campus Wi-Fi remains associated). Connectivity decisions therefore
        // always come from our independent multi-target probes.
        progress.phase("正在验证互联网连通性与校园认证状态", 30);
        let observation = connectivity::observe(&app, &state, &net_info);
        let connection = observation.wait(false).await;
        if connection.cancelled
            || state.network_change_generation.load(Ordering::SeqCst)
                != _schedule_guard.network_generation
        {
            state.is_checking.store(false, Ordering::SeqCst);
            return;
        }
        let system_online = connection.online();
        let is_online = system_online
            && (!connection.require_session || connection.session_online == Some(true));

        rust_log(
            &app,
            &state,
            "网络",
            &format!(
                "[DEBUG] 互联网可用性检测结果: {}",
                if is_online {
                    "连通 (Online)"
                } else {
                    "断开/受限"
                }
            ),
            "debug",
        );

        if network_check_login_operation_superseded(&state, login_operation_generation) {
            rust_log(
                &app,
                &state,
                "网络",
                "本轮网络检测已被新的手动登录或切换账号操作取代，忽略旧检测结果",
                "debug",
            );
            state.is_checking.store(false, Ordering::SeqCst);
            return;
        }

        let latest_network = get_network_info(app.clone(), Some(false));
        if latest_network["interfaceName"] != net_info["interfaceName"]
            || latest_network["ip"] != net_info["ip"]
        {
            state.is_checking.store(false, Ordering::SeqCst);
            schedule_network_change_readiness(app.clone(), state.clone());
            return;
        }
        state.system_online.store(system_online, Ordering::SeqCst);
        if is_online {
            rust_log(
                &app,
                &state,
                "网络",
                "网络检测完毕: 互联网已连通 (Online)",
                "info",
            );
            state.is_checking.store(false, Ordering::SeqCst);
            state.non_campus_count.store(0, Ordering::SeqCst);
            let payload = make_payload("Online", None, &current_ssid, &current_bssid, &current_ip);
            {
                let mut last_state = state.last_network_state.lock().unwrap();
                *last_state = payload.clone();
            }
            let _ = app.emit("network-state-change", payload);
            #[cfg(desktop)]
            refresh_tray_menu(&app, &state);
            return;
        }

        if is_mobile_data {
            rust_log(
                &app,
                &state,
                "网络",
                "移动数据未通过互联网连通性验证；已跳过全部校园网认证网关探测与自动登录",
                "info",
            );
            state.is_checking.store(false, Ordering::SeqCst);
            state.non_campus_count.store(0, Ordering::SeqCst);
            let payload = make_payload("Offline", None, &current_ssid, &current_bssid, &current_ip);
            {
                let mut last_state = state.last_network_state.lock().unwrap();
                *last_state = payload.clone();
            }
            let _ = app.emit("network-state-change", payload);
            #[cfg(desktop)]
            refresh_tray_menu(&app, &state);
            return;
        }

        rust_log(
            &app,
            &state,
            "网络",
            &format!(
                "[DEBUG] 使用 VPN 共存兼容等级 {} 探测校园网网关",
                compatibility.as_str()
            ),
            "debug",
        );
        progress.phase("正在探测校园认证网关，确认登录类型", 65);
        let detection = detect_login_type_details_rust(
            compatibility,
            &current_ssid,
            &transport,
            portal_route_context.as_ref(),
        )
        .await;
        if network_check_login_operation_superseded(&state, login_operation_generation) {
            rust_log(
                &app,
                &state,
                "网络",
                "认证网关探测期间开始了新的登录操作，已取消旧检测的后续自动登录",
                "debug",
            );
            state.is_checking.store(false, Ordering::SeqCst);
            return;
        }
        progress.phase("正在核对网关响应与当前网络身份", 82);
        let type1_requires_maximum = type1_portal_requires_maximum(&detection, compatibility);
        let detected_login_type = if detection.login_ready {
            detection.login_type.clone()
        } else {
            LoginType::Unknown
        };
        #[cfg(not(target_os = "android"))]
        if detected_login_type != LoginType::Unknown {
            // Portal probing is asynchronous. Re-sample the physical route
            // and Wi-Fi identity before any credential can be selected, then
            // require it to match the exact context used by the probe.
            let fresh_network = get_network_info(app.clone(), Some(true));
            let fresh_route_context = portal_route_context_from_network(&fresh_network)
                .ok()
                .flatten();
            let fresh_transport = network_transport(&fresh_network);
            let fresh_ssid = fresh_network
                .get("ssid")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let fresh_bssid = fresh_network
                .get("bssid")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let same_wifi_identity = !transport.eq_ignore_ascii_case("wifi")
                || same_exact_wifi_identity(&current_ssid, &current_bssid, fresh_ssid, fresh_bssid);
            let same_route = portal_route_context.is_some()
                && portal_route_context == fresh_route_context
                && transport.eq_ignore_ascii_case(fresh_transport);
            if same_route && same_wifi_identity {
                trusted_portal_identity = NetworkIdentitySnapshot::capture(&fresh_network);
                current_ssid = fresh_ssid.to_string();
                current_bssid = fresh_bssid.to_string();
                current_ip = fresh_network
                    .get("ip")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&current_ip)
                    .to_string();
                portal_route_context = fresh_route_context;
                identity_fresh = true;
            } else {
                identity_fresh = false;
                rust_log(
                    &app,
                    &state,
                    "安全",
                    "认证网关探测期间物理接口、IP 或 Wi-Fi 身份发生变化，已阻止本轮自动登录",
                    "error",
                );
            }
        }
        #[cfg(target_os = "android")]
        if !full_details
            && transport.eq_ignore_ascii_case("wifi")
            && detected_login_type != LoginType::Unknown
        {
            let fresh_network = get_network_info(app.clone(), Some(true));
            let captured_network_id = net_info
                .get("networkId")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let fresh_network_id = fresh_network
                .get("networkId")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let same_network =
                !captured_network_id.is_empty() && captured_network_id == fresh_network_id;
            current_ssid = fresh_network
                .get("ssid")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            current_bssid = fresh_network
                .get("bssid")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            current_ip = fresh_network
                .get("ip")
                .and_then(|value| value.as_str())
                .unwrap_or(&current_ip)
                .to_string();
            identity_fresh = fresh_network
                .get("identityFresh")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
                && same_network;
            rust_log(
                &app,
                &state,
                "安全",
                if !same_network {
                    "读取 Wi-Fi 身份期间网络已经切换，已阻止本轮自动登录"
                } else if identity_fresh {
                    "检测到认证网关，已在发送凭据前重新读取当前 Wi-Fi 身份"
                } else {
                    "检测到认证网关，但系统未能返回有效的当前 SSID/BSSID"
                },
                if identity_fresh { "debug" } else { "error" },
            );
        }
        let profile = {
            let cfg = state.config.read().unwrap();
            matching_network_profile(&cfg, &current_ssid, &current_bssid, &detected_login_type)
        };
        let login_type = profile
            .as_ref()
            .and_then(|item| login_type_from_profile(&item.login_type))
            .unwrap_or_else(|| detected_login_type.clone());
        if let Some(profile) = profile.as_ref() {
            rust_log(
                &app,
                &state,
                "网络档案",
                &format!("已匹配网络档案“{}”，使用其账号顺序与登录策略", profile.name),
                "info",
            );
            let profile_interval = if is_bg {
                profile.check_interval_bg
            } else {
                profile.check_interval
            };
            if let Some(interval) = profile_interval.filter(|value| *value >= 5) {
                state.countdown.store(interval, Ordering::SeqCst);
            }
        }
        rust_log(
            &app,
            &state,
            "网络",
            &format!(
                "[DEBUG] 检测到校园网环境判定: {}",
                if login_type != LoginType::Unknown {
                    "需要登录认证"
                } else {
                    "非校园网/完全离线"
                }
            ),
            "debug",
        );

        if type1_requires_maximum {
            state.non_campus_count.store(0, Ordering::SeqCst);
            rust_log(
                &app,
                &state,
                "网络",
                TYPE1_MAXIMUM_MODE_REQUIRED_MESSAGE,
                "info",
            );
            let mut payload = make_payload(
                "BjutCampus",
                Some(&LoginType::Type1),
                &current_ssid,
                &current_bssid,
                &current_ip,
            );
            if let Some(object) = payload.as_object_mut() {
                object.insert(
                    "loginMessage".to_string(),
                    serde_json::json!(TYPE1_MAXIMUM_MODE_REQUIRED_MESSAGE),
                );
            }
            *state.last_network_state.lock().unwrap() = payload.clone();
            let _ = app.emit("network-state-change", payload);
        } else {
            match login_type {
                LoginType::Unknown => {
                    state.non_campus_count.store(0, Ordering::SeqCst);
                    rust_log(
                        &app,
                        &state,
                        "网络",
                        "网络检测完毕: 离线或非校园网 (Offline)",
                        "info",
                    );
                    let payload =
                        make_payload("Offline", None, &current_ssid, &current_bssid, &current_ip);
                    {
                        let mut last_state = state.last_network_state.lock().unwrap();
                        *last_state = payload.clone();
                    }
                    let _ = app.emit("network-state-change", payload);
                }
                _ => {
                    rust_log(
                        &app,
                        &state,
                        "网络",
                        &format!("检测到校园网登录页面 (登录类型: {:?})", login_type),
                        "info",
                    );
                    let mut login_succeeded = false;
                    let mut login_failure_message: Option<String> = None;
                    let auto_login_paused = state.auto_login_paused_until.load(Ordering::SeqCst)
                        > chrono::Utc::now().timestamp();
                    let auto_login_enabled = {
                        let cfg = state.config.read().unwrap();
                        profile_auto_login_enabled(profile.as_ref(), &login_type, cfg.auto_login)
                    };
                    if auto_login_enabled && !auto_login_paused {
                        #[cfg(target_os = "android")]
                        {
                            // Re-read only the non-location-protected network/IP
                            // identity immediately before credentials are used. A
                            // Network object can be replaced during a long portal
                            // probe even when SSID/BSSID were coherent at capture.
                            let latest_network = get_network_info(app.clone(), Some(false));
                            let captured_network_id = net_info
                                .get("networkId")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("");
                            let latest_network_id = latest_network
                                .get("networkId")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("");
                            let latest_ip = latest_network
                                .get("ip")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("");
                            if captured_network_id.is_empty()
                                || captured_network_id != latest_network_id
                                || latest_ip != current_ip
                            {
                                identity_fresh = false;
                                rust_log(
                                &app,
                                &state,
                                "安全",
                                "发送凭据前检测到 Android Network 或物理 IPv4 已变化，已取消本轮自动登录",
                                "error",
                            );
                            }
                        }
                        let (whitelist, blacklist) = {
                            let cfg = state.config.read().unwrap();
                            (cfg.whitelist.clone(), cfg.blacklist.clone())
                        };
                        let proceed = match automatic_login_network_allowed(NetworkTrustInput {
                            login_type: &login_type,
                            ssid: &current_ssid,
                            bssid: &current_bssid,
                            ip: &current_ip,
                            transport: &transport,
                            identity_fresh,
                            whitelist: &whitelist,
                            blacklist: &blacklist,
                        }) {
                            Ok(()) => true,
                            Err(reason) => {
                                login_failure_message = Some(format!("自动登录已阻止：{reason}"));
                                rust_log(
                                    &app,
                                    &state,
                                    "安全",
                                    &format!("自动登录已阻止: {reason}"),
                                    "error",
                                );
                                false
                            }
                        };
                        if proceed {
                            progress.phase("已确认校园网登录类型，正在执行自动登录", 92);
                            let accounts = {
                                let cfg = state.config.read().unwrap();
                                cfg.accounts.clone()
                            };
                            let mut active_accounts = Vec::new();
                            for account in accounts_for_profile(accounts, profile.as_ref()) {
                                match account_attempt_allowed(&state, &account.user) {
                                    Ok(()) => active_accounts.push(account),
                                    Err(remaining) => rust_log(
                                        &app,
                                        &state,
                                        "账号健康",
                                        &format!(
                                            "账号 {} 仍在冷却中（剩余 {} 秒），跳过本次尝试",
                                            account.user, remaining
                                        ),
                                        "info",
                                    ),
                                }
                            }
                            if active_accounts.is_empty() {
                                login_failure_message =
                                    Some("没有带已保存密码且当前可尝试的账号".to_string());
                                rust_log(
                                    &app,
                                    &state,
                                    "网络",
                                    "未配置带已保存密码的有效账号，跳过自动登录",
                                    "error",
                                );
                            } else {
                                let mut success = false;
                                let mut result_uncertain = false;
                                #[cfg(not(target_os = "android"))]
                                let mut aborted_for_network_change = false;
                                #[cfg(target_os = "android")]
                                let aborted_for_network_change = false;
                                let mut aborted_for_new_login_operation = false;
                                for acc in active_accounts {
                                    if network_check_login_operation_superseded(
                                        &state,
                                        login_operation_generation,
                                    ) {
                                        aborted_for_new_login_operation = true;
                                        break;
                                    }
                                    #[cfg(not(target_os = "android"))]
                                    {
                                        // A rejected account can keep the request in flight long
                                        // enough for a roam, DHCP renewal, or interface switch.
                                        // Reconfirm the exact route and Wi-Fi identity before every
                                        // subsequent credential-bearing request.
                                        let latest_network =
                                            get_network_info(app.clone(), Some(true));
                                        let route_still_fresh =
                                            network_identity_is_fresh(&latest_network)
                                                && ensure_same_network_identity(
                                                    &trusted_portal_identity,
                                                    &latest_network,
                                                )
                                                .is_ok()
                                                && portal_route_context_from_network(
                                                    &latest_network,
                                                )
                                                .ok()
                                                .flatten()
                                                    == portal_route_context;
                                        if !route_still_fresh {
                                            let message = "账号重试前检测到物理接口、IP 或 Wi-Fi 身份变化，已停止发送下一组账号密码";
                                            login_failure_message = Some(message.to_string());
                                            aborted_for_network_change = true;
                                            rust_log(&app, &state, "安全", message, "error");
                                            break;
                                        }
                                    }
                                    rust_log(
                                        &app,
                                        &state,
                                        "网络",
                                        &format!("尝试使用账号 {} 自动登录...", acc.user),
                                        "info",
                                    );
                                    let login_guard = state.login_request_lock.lock().await;
                                    if network_check_login_operation_superseded(
                                        &state,
                                        login_operation_generation,
                                    ) {
                                        drop(login_guard);
                                        aborted_for_new_login_operation = true;
                                        break;
                                    }
                                    connectivity::invalidate(&app, &state);
                                    let login_result = login_to_campus_network_rust(
                                        login_type.clone(),
                                        &acc.user,
                                        &acc.pass,
                                        compatibility,
                                        portal_route_context.as_ref(),
                                    )
                                    .await;
                                    drop(login_guard);
                                    if network_check_login_operation_superseded(
                                        &state,
                                        login_operation_generation,
                                    ) {
                                        aborted_for_new_login_operation = true;
                                        break;
                                    }
                                    match login_result {
                                        Ok((true, msg)) => {
                                            record_account_success(&app, &state, &acc.user);
                                            rust_log(
                                                &app,
                                                &state,
                                                "网络",
                                                &format!("登录成功: {}", msg),
                                                "success",
                                            );
                                            if automatic_login_result_notifications_enabled(&state)
                                            {
                                                let _ = show_native_notification(
                                                    &app,
                                                    "自动登录成功",
                                                    &format!("账号: {}", acc.user),
                                                );
                                            }
                                            success = true;
                                            login_succeeded = true;
                                            #[cfg(target_os = "android")]
                                            report_android_campus_wifi_connected(&net_info);
                                            break;
                                        }
                                        Ok((false, msg)) => {
                                            match classify_portal_login_failure(&msg) {
                                                PortalLoginFailureDisposition::CredentialRejected => {
                                                    login_failure_message =
                                                        Some(format!("自动登录失败：{msg}"));
                                                    record_account_failure(
                                                        &app,
                                                        &state,
                                                        &acc.user,
                                                        &msg,
                                                    );
                                                    rust_log(
                                                        &app,
                                                        &state,
                                                        "网络",
                                                        &format!("登录失败: {msg}"),
                                                        "error",
                                                    );
                                                }
                                                PortalLoginFailureDisposition::SessionAlreadyOnline => {
                                                    let online = connectivity::observe(&app, &state, &net_info).wait(false).await.online();
                                                    if online {
                                                        success = true;
                                                        login_succeeded = true;
                                                        login_failure_message = None;
                                                        rust_log(
                                                            &app,
                                                            &state,
                                                            "网络",
                                                            "认证网关报告当前 IP 已在线，且独立互联网探测已通过；停止遍历其他账号",
                                                            "success",
                                                        );
                                                    } else {
                                                        result_uncertain = true;
                                                        login_failure_message = Some(format!(
                                                            "认证网关报告当前 IP 已在线，但互联网尚未验证：{msg}"
                                                        ));
                                                        state.auto_login_paused_until.store(
                                                            chrono::Utc::now().timestamp() + 10,
                                                            Ordering::SeqCst,
                                                        );
                                                        rust_log(
                                                            &app,
                                                            &state,
                                                            "网络",
                                                            "认证网关报告当前 IP 已在线；为避免向同一会话遍历全部账号，本轮已停止并将在稍后重新检测互联网",
                                                            "info",
                                                        );
                                                    }
                                                    break;
                                                }
                                                PortalLoginFailureDisposition::StopTraversal => {
                                                    result_uncertain = true;
                                                    login_failure_message =
                                                        Some(format!("自动登录已停止：{msg}"));
                                                    rust_log(
                                                        &app,
                                                        &state,
                                                        "网络",
                                                        &format!(
                                                            "认证网关返回了非账号特定错误，已停止遍历其他账号：{msg}"
                                                        ),
                                                        "error",
                                                    );
                                                    break;
                                                }
                                            }
                                        }
                                        Err(err) => {
                                            if login_result_is_ambiguous(&err) {
                                                login_failure_message = Some(err.clone());
                                                result_uncertain = true;
                                                state.auto_login_paused_until.store(
                                                    chrono::Utc::now().timestamp() + 60,
                                                    Ordering::SeqCst,
                                                );
                                                rust_log(
                                                &app,
                                                &state,
                                                "网络",
                                                &format!(
                                                    "账号 {} 的登录请求已发送，但结果无法确认；为避免重复认证，本轮不再尝试其他账号：{}",
                                                    acc.user, err
                                                ),
                                                "error",
                                            );
                                                break;
                                            }
                                            let _ = connectivity::observe(&app, &state, &net_info)
                                                .wait(false)
                                                .await;
                                            result_uncertain = true;
                                            login_failure_message =
                                                Some(format!("自动登录请求失败：{err}"));
                                            rust_log(
                                                &app,
                                                &state,
                                                "网络",
                                                &format!("认证响应未确认，已停止遍历账号：{err}"),
                                                "error",
                                            );
                                            break;
                                        }
                                    }
                                }
                                if !success
                                    && !result_uncertain
                                    && !aborted_for_network_change
                                    && !aborted_for_new_login_operation
                                {
                                    if login_failure_message.is_none() {
                                        login_failure_message =
                                            Some("所有账号均未能完成登录".to_string());
                                    }
                                    rust_log(
                                        &app,
                                        &state,
                                        "网络",
                                        "所有账号登录尝试完毕，均未成功",
                                        "error",
                                    );
                                }
                            }
                        }
                    } else if auto_login_paused {
                        login_failure_message = Some("自动登录已临时暂停，请稍后重试".to_string());
                        rust_log(&app, &state, "网络", "自动登录已临时暂停，忽略重连", "info");
                    } else {
                        rust_log(&app, &state, "网络", "自动登录未开启，忽略重连", "info");
                    }
                    if network_check_login_operation_superseded(&state, login_operation_generation)
                    {
                        rust_log(
                            &app,
                            &state,
                            "网络",
                            "旧检测的自动登录阶段已被新的手动登录操作取代，不再覆盖控制台状态",
                            "debug",
                        );
                        state.is_checking.store(false, Ordering::SeqCst);
                        return;
                    }
                    if login_succeeded {
                        state.non_campus_count.store(0, Ordering::SeqCst);
                    } else if is_bg {
                        let count = state.non_campus_count.fetch_add(1, Ordering::SeqCst) + 1;
                        rust_log(
                            &app,
                            &state,
                            "网络",
                            &format!("[DEBUG] 后台检测为非校园网环境，当前连续次数: {}/5", count),
                            "debug",
                        );
                        if count >= 5 && !state.config.read().unwrap().adaptive_network_checks {
                            rust_log(&app, &state, "网络", "后台连续5次检测到校园网登录页面（或自动登录失败），进入自动休眠模式以省电。返回前台时将自动恢复。", "info");
                            state.is_suspended.store(true, Ordering::SeqCst);
                        }
                    } else {
                        state.non_campus_count.store(0, Ordering::SeqCst);
                    }
                    let mut payload = if login_succeeded {
                        make_payload(
                            "Online",
                            Some(&login_type),
                            &current_ssid,
                            &current_bssid,
                            &current_ip,
                        )
                    } else {
                        make_payload(
                            "BjutCampus",
                            Some(&login_type),
                            &current_ssid,
                            &current_bssid,
                            &current_ip,
                        )
                    };
                    if !login_succeeded {
                        if let (Some(message), Some(object)) =
                            (login_failure_message, payload.as_object_mut())
                        {
                            object.insert("loginMessage".to_string(), serde_json::json!(message));
                        }
                    }
                    {
                        let mut last_state = state.last_network_state.lock().unwrap();
                        *last_state = payload.clone();
                    }
                    let _ = app.emit("network-state-change", payload);
                }
            }
        }
        state.is_checking.store(false, Ordering::SeqCst);
        #[cfg(desktop)]
        refresh_tray_menu(&app, &state);
    });
}

#[tauri::command]
fn export_config_backup(
    state: tauri::State<Arc<AppState>>,
    passphrase: String,
    ui_preferences: serde_json::Value,
    scope: Option<BackupScope>,
) -> Result<ConfigBackupExport, String> {
    let scope = scope.unwrap_or_default().validate()?;
    if !ui_preferences.is_object() && !ui_preferences.is_null() {
        return Err("界面偏好设置格式无效".to_string());
    }
    if serde_json::to_vec(&ui_preferences)
        .map_err(|error| error.to_string())?
        .len()
        > 64 * 1024
    {
        return Err("界面偏好设置超过安全大小限制".to_string());
    }
    let current = state.config.read().unwrap().clone();
    let mut config = merge_backup_config(
        &serde_json::from_value(serde_json::json!({})).expect("default configuration"),
        current,
        scope,
    );
    // Sessions and payment recovery records have their own lifecycles and can
    // expire while a backup is stored. Export only durable configuration and
    // credentials; importing must never resurrect an old authenticated or
    // financial operation context.
    config.campus_service_sessions.clear();
    config.recharge_transactions = recharge_state::RechargeJournal::default();
    validate_imported_config(&config)?;
    let (account_count, password_count, missing_password_accounts) = config_backup_counts(&config);
    let plaintext = ConfigBackupPlaintext {
        format: "BJUT-AL-CONFIG".to_string(),
        version: CONFIG_BACKUP_VERSION,
        config,
        scope,
        ui_preferences: if scope.settings {
            ui_preferences
        } else {
            serde_json::Value::Null
        },
    };
    let mut passphrase = passphrase.into_bytes();
    let payload = encrypt_config_backup_payload(&plaintext, &passphrase);
    passphrase.fill(0);
    Ok(ConfigBackupExport {
        payload: payload?,
        account_count,
        password_count,
        missing_password_accounts,
    })
}

#[tauri::command]
fn import_config_backup(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    payload: String,
    passphrase: String,
    scope: Option<BackupScope>,
) -> Result<ConfigBackupImport, String> {
    let scope = scope.unwrap_or_default().validate()?;
    let mut passphrase = passphrase.into_bytes();
    let plaintext = decrypt_config_backup_payload(payload.trim(), &passphrase);
    passphrase.fill(0);
    let plaintext = plaintext?;
    if (scope.settings && !plaintext.scope.settings)
        || (scope.accounts && !plaintext.scope.accounts)
    {
        return Err("备份不包含所选内容，请调整导入选项".into());
    }
    validate_imported_config(&plaintext.config)?;
    let current = state.config.read().unwrap().clone();
    let merged = merge_backup_config(&current, plaintext.config, scope);
    save_config(&app, &state, merged)?;
    let saved = state.config.read().unwrap();
    let (account_count, password_count, missing_password_accounts) = config_backup_counts(&saved);
    Ok(ConfigBackupImport {
        ui_preferences: if scope.settings {
            plaintext.ui_preferences
        } else {
            serde_json::Value::Null
        },
        account_count,
        password_count,
        missing_password_accounts,
    })
}

#[tauri::command]
fn sync_config(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    config: AppConfig,
) -> Result<(), String> {
    let mut config = config;
    config.preferred_interface = state.config.read().unwrap().preferred_interface.clone();
    if let Err(error) = save_config(&app, &state, config) {
        rust_log(
            &app,
            &state,
            "配置",
            &format!("配置持久化失败: {error}"),
            "error",
        );
        return Err(error);
    }
    rust_log(&app, &state, "配置", "配置已写入安全存储", "debug");
    let is_bg = state.is_in_background.load(Ordering::SeqCst);
    let current_val = state.countdown.load(Ordering::SeqCst);
    let new_cfg = state.config.read().unwrap();
    let configured_interval = if is_bg {
        new_cfg.check_interval_bg
    } else {
        new_cfg.check_interval
    };
    let mobile_data = is_mobile_data_network(&state.last_network_state.lock().unwrap());
    let new_interval = if mobile_data {
        mobile_data_check_interval(configured_interval, is_bg)
    } else {
        configured_interval
    };
    if current_val > new_interval {
        state.countdown.store(new_interval, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
fn get_app_config(state: tauri::State<Arc<AppState>>) -> serde_json::Value {
    let config = state.config.read().unwrap().clone();
    let mut value = serde_json::to_value(&config).unwrap_or_default();
    if let Some(accounts) = value
        .get_mut("accounts")
        .and_then(serde_json::Value::as_array_mut)
    {
        for (view, saved) in accounts.iter_mut().zip(config.accounts.iter()) {
            if let Some(object) = view.as_object_mut() {
                object.insert("pass".to_string(), serde_json::Value::String(String::new()));
                object.insert(
                    "hasPassword".to_string(),
                    serde_json::Value::Bool(!saved.pass.is_empty()),
                );
            }
        }
    }
    if let Some(object) = value.as_object_mut() {
        object.insert("campusServiceSessions".to_string(), serde_json::json!([]));
        object.insert("rechargeTransactions".to_string(), serde_json::json!([]));
    }
    value
}

fn credential_snapshot_fingerprint(config: &AppConfig, users: &[String]) -> Option<String> {
    use base64::Engine;
    use sha2::Digest;

    #[derive(serde::Serialize)]
    struct CredentialSnapshot<'a> {
        user: &'a str,
        pass: &'a str,
    }

    if users.is_empty() {
        return None;
    }
    let mut snapshot = Vec::with_capacity(users.len());
    for user in users {
        let account = config
            .accounts
            .iter()
            .find(|account| account.user == *user)?;
        if account.pass.is_empty() {
            return None;
        }
        snapshot.push(CredentialSnapshot {
            user: &account.user,
            pass: &account.pass,
        });
    }
    let serialized = serde_json::to_vec(&snapshot).ok()?;
    let digest = sha2::Sha256::digest(serialized);
    Some(base64::engine::general_purpose::STANDARD.encode(digest))
}

#[tauri::command]
fn verify_legacy_credential_fingerprint(
    state: tauri::State<Arc<AppState>>,
    users: Vec<String>,
    fingerprint: String,
) -> bool {
    if fingerprint.trim().is_empty() {
        return false;
    }
    let config = state.config.read().unwrap();
    credential_snapshot_fingerprint(&config, &users).is_some_and(|actual| actual == fingerprint)
}

#[tauri::command]
fn get_account_password(
    state: tauri::State<Arc<AppState>>,
    user: String,
) -> Result<String, String> {
    let user = user.trim();
    let config = state.config.read().unwrap();
    config
        .accounts
        .iter()
        .find(|account| account.user == user)
        .map(|account| account.pass.clone())
        .filter(|password| !password.is_empty())
        .ok_or_else(|| "该账号没有可读取的已保存密码".to_string())
}

#[tauri::command]
fn get_credential_storage_status(state: tauri::State<Arc<AppState>>) -> String {
    state.credential_storage_status.lock().unwrap().clone()
}

fn credential_backend_name() -> &'static str {
    if cfg!(target_os = "android") {
        "Android Keystore (AES-GCM)"
    } else if cfg!(target_os = "macos") {
        "macOS 本地加密文件 (AES-GCM)"
    } else if cfg!(target_os = "windows") {
        "Windows Credential Manager"
    } else if cfg!(target_os = "linux") {
        "Linux Secret Service"
    } else {
        "安全凭据存储"
    }
}

#[tauri::command]
fn get_credential_storage_health(state: tauri::State<Arc<AppState>>) -> CredentialStorageHealth {
    let status = state.credential_storage_status.lock().unwrap().clone();
    let config = state.config.read().unwrap();
    let missing_password_accounts: Vec<String> = config
        .accounts
        .iter()
        .filter(|account| account.pass.is_empty())
        .map(|account| account.user.clone())
        .collect();
    let saved_accounts = config
        .accounts
        .len()
        .saturating_sub(missing_password_accounts.len());
    let message = match status.as_str() {
        "available" if missing_password_accounts.is_empty() => {
            "安全凭据存储工作正常，所有账号均已保存密码"
        }
        "available" => "安全凭据存储可用，但部分账号仍需补录密码",
        "missing" => "安全凭据存储可用，当前尚未保存凭据",
        "error" => "安全凭据存储暂时不可读；应用已阻止空密码覆盖",
        _ => "安全凭据存储状态尚未完成初始化",
    }
    .to_string();
    CredentialStorageHealth {
        status,
        backend: credential_backend_name().to_string(),
        persistent: true,
        saved_accounts,
        missing_password_accounts,
        message,
    }
}

#[tauri::command]
fn get_account_health(state: tauri::State<Arc<AppState>>) -> Vec<AccountHealthView> {
    current_account_health_views(&state)
}

#[tauri::command]
fn reset_account_health(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    user: Option<String>,
) {
    let snapshot = {
        let mut health = state.account_health.lock().unwrap();
        if let Some(user) = user {
            health.remove(&user);
        } else {
            health.clear();
        }
        health.clone()
    };
    persist_account_health(&app, &snapshot);
    emit_account_health(&app, &state);
}

fn make_diagnostic_step(
    id: &str,
    label: &str,
    started: std::time::Instant,
    status: &str,
    message: String,
) -> DiagnosticStep {
    DiagnosticStep {
        id: id.to_string(),
        label: label.to_string(),
        status: status.to_string(),
        message,
        duration_ms: started.elapsed().as_millis(),
    }
}

fn emit_network_diagnostic_progress(
    app: &tauri::AppHandle,
    run_id: &Option<String>,
    percent: u8,
    label: &str,
) {
    let _ = app.emit(
        "network-diagnostic-progress",
        serde_json::json!({
            "percent": percent.min(100),
            "label": label,
            "runId": run_id,
        }),
    );
}

#[tauri::command]
async fn restart_lgn_adapter(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    target: AdapterRestartTarget,
) -> Result<String, String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, target);
        Err("一键重启有线适配器目前仅支持 macOS".to_string())
    }
    #[cfg(target_os = "macos")]
    {
        if state.manual_login_in_progress.swap(true, Ordering::SeqCst) {
            return Err("已有登录、账号切换或适配器重启正在进行，请稍候".to_string());
        }
        state
            .login_operation_generation
            .fetch_add(1, Ordering::SeqCst);
        connectivity::invalidate(&app, &state);
        let operation_guard = ManualLoginOperationGuard {
            generation: &state.login_operation_generation,
            active: &state.manual_login_in_progress,
        };
        let login_guard = state.login_request_lock.lock().await;
        let network = get_network_info(app.clone(), Some(false));
        network_repair::validate_target(&target, &network)?;
        let route = portal_route_context_from_network(&network)?;
        let compatibility = effective_vpn_compatibility(&state.config.read().unwrap());
        let configuration_issue = serde_json::from_value::<macos_network::LgnLinkConfiguration>(
            network
                .get("lgnLinkConfiguration")
                .cloned()
                .unwrap_or_default(),
        )
        .is_ok_and(|configuration| configuration.resembles_reported_ipv6_failure());
        if diagnose_lgn_ipv6_rust(compatibility, route.as_ref())
            .await
            .is_ok()
            && !configuration_issue
        {
            return Ok("IPv6 探测已恢复，无需重启适配器".to_string());
        }
        rust_log(
            &app,
            &state,
            "网络",
            &format!(
                "用户请求重启有线适配器 {}，等待系统授权",
                target.interface_name
            ),
            "info",
        );
        let interface_name = target.interface_name.clone();
        let mut result =
            tokio::task::spawn_blocking(move || network_repair::restart_ethernet(&target))
                .await
                .map_err(|error| format!("适配器重启任务失败：{error}"))?;
        if let Ok(message) = &mut result {
            // Wait for this adapter, not an already-connected Wi-Fi, before
            // the frontend refreshes the diagnostic report after the restart.
            let mut address_ready = false;
            for _ in 0..10 {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if usable_physical_ipv4(&macos_ipv4_for_interface(&interface_name)).is_some() {
                    address_ready = true;
                    break;
                }
            }
            if !address_ready {
                message.push_str("；10 秒内尚未取得有线 IPv4，请稍后再次诊断");
            }
        }
        match &result {
            Ok(message) => rust_log(&app, &state, "网络", message, "success"),
            Err(error) => rust_log(&app, &state, "网络", error, "error"),
        }
        drop(login_guard);
        drop(operation_guard);
        if result.is_ok() {
            schedule_network_change_readiness(app, state.inner().clone());
        }
        result
    }
}

#[tauri::command]
fn get_logs(state: tauri::State<Arc<AppState>>) -> Vec<LogEntry> {
    state.logs.lock().unwrap().clone()
}

#[tauri::command]
fn get_log_text(app: tauri::AppHandle) -> String {
    std::fs::read_to_string(get_log_path(&app)).unwrap_or_default()
}

#[tauri::command]
fn export_logs(app: tauri::AppHandle) -> Result<String, String> {
    let source = get_log_path(&app);
    let metadata = std::fs::metadata(&source).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "当前没有可导出的日志".to_string()
        } else {
            format!("读取日志失败：{error}")
        }
    })?;
    if metadata.len() == 0 {
        return Err("当前没有可导出的日志".to_string());
    }
    let directory = app
        .path()
        .download_dir()
        .or_else(|_| app.path().document_dir())
        .map_err(|error| format!("无法定位导出目录：{error}"))?;
    std::fs::create_dir_all(&directory).map_err(|error| format!("无法创建导出目录：{error}"))?;
    let filename = format!(
        "BJUT-AL-logs-{}.log",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let destination = directory.join(filename);
    std::fs::copy(&source, &destination).map_err(|error| format!("导出日志失败：{error}"))?;
    Ok(destination.to_string_lossy().into_owned())
}

#[tauri::command]
fn export_billing_csv(app: tauri::AppHandle, kind: String, csv: String) -> Result<String, String> {
    let stem = match kind.as_str() {
        "usage" => "usage",
        "monthly" => "monthly",
        "payments" => "payments",
        "operations" => "operations",
        "stopLogs" => "stop-logs",
        "reopenLogs" => "reopen-logs",
        "packageLogs" => "package-logs",
        _ => return Err("不支持的账单导出类型".to_string()),
    };
    if csv.trim().is_empty() {
        return Err("当前没有可导出的账单记录".to_string());
    }
    if csv.len() > 32 * 1024 * 1024 {
        return Err("账单导出内容超过 32 MiB，请缩小查询日期范围".to_string());
    }

    #[cfg(target_os = "android")]
    let directory = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("无法定位导出缓存目录：{error}"))?
        .join("exports");
    #[cfg(not(target_os = "android"))]
    let directory = app
        .path()
        .download_dir()
        .or_else(|_| app.path().document_dir())
        .map_err(|error| format!("无法定位导出目录：{error}"))?;

    std::fs::create_dir_all(&directory).map_err(|error| format!("无法创建导出目录：{error}"))?;
    let filename = format!(
        "BJUT-AL-billing-{stem}-{}.csv",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let destination = directory.join(filename);
    std::fs::write(&destination, csv.as_bytes())
        .map_err(|error| format!("写入账单 CSV 失败：{error}"))?;
    Ok(destination.to_string_lossy().into_owned())
}

#[tauri::command]
fn clear_all_logs(app: tauri::AppHandle, state: tauri::State<Arc<AppState>>) {
    state.logs.lock().unwrap().clear();
    let p = get_log_path(&app);
    let _ = std::fs::remove_file(p);
    #[cfg(target_os = "android")]
    if let Some(parent) = get_log_path(&app).parent() {
        let _ = std::fs::remove_file(parent.join(ANDROID_KEEPALIVE_JOURNAL));
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let file_name = file_name.to_string_lossy();
                if file_name.starts_with("keepalive-journal.importing")
                    && file_name.ends_with(".log")
                {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

#[tauri::command]
fn get_countdown_status(state: tauri::State<Arc<AppState>>) -> serde_json::Value {
    let is_chk = state.is_checking.load(Ordering::SeqCst);
    let is_susp = state.is_suspended.load(Ordering::SeqCst);
    let current_countdown = state.countdown.load(Ordering::SeqCst);
    let status = if is_chk {
        "checking"
    } else if is_susp {
        "suspended"
    } else {
        "ticking"
    };
    serde_json::json!({
        "status": status,
        "seconds": current_countdown
    })
}

#[tauri::command]
fn trigger_manual_check(app: tauri::AppHandle, state: tauri::State<Arc<AppState>>) {
    state.network_schedule.lock().unwrap().reset();
    rust_log(
        &app,
        &state,
        "网络",
        "收到手动连通性检测请求，开始检测...",
        "info",
    );
    state.is_suspended.store(false, Ordering::SeqCst);
    state.non_campus_count.store(0, Ordering::SeqCst);
    let app_clone = app.clone();
    let state_clone = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        trigger_network_check(app_clone, state_clone, true).await;
    });
}

#[tauri::command]
async fn evaluate_manual_network_trust(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    login_type_override: Option<String>,
) -> Result<NetworkTrustEvaluation, String> {
    let network = get_network_info(app, Some(true));
    #[cfg(target_os = "android")]
    let wifi_route_guard = AndroidWifiRouteGuard::bind_if_required(&network);
    #[cfg(target_os = "android")]
    if wifi_route_guard.failed() {
        return Err("无法将认证请求绑定到当前校园 Wi-Fi；已停止使用 VPN/移动数据路径".to_string());
    }
    let transport = network_transport(&network);
    let ssid = network
        .get("ssid")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let bssid = network
        .get("bssid")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let ip = network
        .get("ip")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let config = state.config.read().unwrap().clone();
    let compatibility = effective_vpn_compatibility(&config);
    // A same-interface route is preferred when available. Unlike portal login,
    // this read-only request may safely fall back to the system route so a VPN
    // connected to the accounting network can still expose session data.
    let portal_route_context = portal_route_context_from_network(&network).ok().flatten();
    let detection = detect_login_type_details_rust(
        compatibility,
        ssid,
        transport,
        portal_route_context.as_ref(),
    )
    .await;
    let detected = if detection.login_ready {
        detection.login_type.clone()
    } else {
        LoginType::Unknown
    };
    let login_type = parse_login_type_override(login_type_override.as_deref()).unwrap_or(detected);
    let result = if login_type == LoginType::Unknown {
        network_trust::NetworkTrustResult {
            decision: NetworkTrustDecision::Blocked,
            reason: if type1_portal_requires_maximum(&detection, compatibility) {
                TYPE1_MAXIMUM_MODE_REQUIRED_MESSAGE.to_string()
            } else {
                "未检测到可确认的校园网认证协议".to_string()
            },
            network_key: network_trust::network_key(ssid, bssid),
        }
    } else {
        evaluate_network_trust(NetworkTrustInput {
            login_type: &login_type,
            ssid,
            bssid,
            ip,
            transport,
            identity_fresh: network_identity_is_fresh(&network),
            whitelist: &config.whitelist,
            blacklist: &config.blacklist,
        })
    };
    Ok(NetworkTrustEvaluation {
        decision: match result.decision {
            NetworkTrustDecision::Allowed => "allowed",
            NetworkTrustDecision::Blocked => "blocked",
            NetworkTrustDecision::NeedsConfirmation => "confirm",
        }
        .to_string(),
        reason: result.reason,
        network_key: result.network_key,
        ssid: ssid.to_string(),
        bssid: bssid.to_string(),
        ip: ip.to_string(),
    })
}

#[tauri::command]
fn set_current_network_trust(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    trusted: bool,
    expected_network_key: Option<String>,
) -> Result<NetworkTrustLists, String> {
    let network = get_network_info(app.clone(), Some(true));
    let transport = network_transport(&network);
    let ssid = network
        .get("ssid")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let bssid = network
        .get("bssid")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if !transport.eq_ignore_ascii_case("wifi")
        || !network_identity_is_fresh(&network)
        || ssid.trim().is_empty()
        || bssid.trim().is_empty()
        || bssid == "00:00:00:00:00:00"
    {
        return Err("无法确认当前 Wi-Fi 的有效 SSID/BSSID，未更改信任设置".to_string());
    }
    if expected_network_key
        .as_deref()
        .is_some_and(|expected| network_trust::network_key(ssid, bssid) != expected)
    {
        return Err("确认期间网络已经切换，未更改信任设置".to_string());
    }
    let mut updated = state.config.read().unwrap().clone();
    let key = set_network_trust(
        &mut updated.whitelist,
        &mut updated.blacklist,
        ssid,
        bssid,
        trusted,
    );
    save_secure_config_verified(&app, &updated)?;
    let lists = NetworkTrustLists {
        whitelist: updated.whitelist.clone(),
        blacklist: updated.blacklist.clone(),
    };
    *state.config.write().unwrap() = updated;
    rust_log(
        &app,
        &state,
        "安全",
        &format!(
            "已将网络 {key} 设置为{}",
            if trusted { "信任" } else { "拒绝" }
        ),
        "info",
    );
    Ok(lists)
}

#[tauri::command]
fn set_named_network_trust(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    trusted: bool,
    ssid: String,
    bssid: String,
) -> Result<NetworkTrustLists, String> {
    let ssid = ssid.trim();
    let bssid = bssid.trim();
    if ssid.is_empty() || ssid.len() > 32 {
        return Err("Wi-Fi 名称不能为空，且 UTF-8 编码后不能超过 32 字节".to_string());
    }
    if !network_trust::usable_wifi_bssid(bssid) {
        return Err("请输入完整有效的 BSSID，例如 AA:BB:CC:DD:EE:FF".to_string());
    }
    let mut updated = state.config.read().unwrap().clone();
    let key = set_network_trust(
        &mut updated.whitelist,
        &mut updated.blacklist,
        ssid,
        bssid,
        trusted,
    );
    save_secure_config_verified(&app, &updated)?;
    let lists = NetworkTrustLists {
        whitelist: updated.whitelist.clone(),
        blacklist: updated.blacklist.clone(),
    };
    *state.config.write().unwrap() = updated;
    rust_log(
        &app,
        &state,
        "安全",
        &format!(
            "已手动将网络 {key} 设置为{}",
            if trusted { "信任" } else { "拒绝" }
        ),
        "info",
    );
    Ok(lists)
}

#[tauri::command]
fn remove_saved_network_trust(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    network_key: String,
) -> Result<NetworkTrustLists, String> {
    let mut updated = state.config.read().unwrap().clone();
    if !remove_network_trust(&mut updated.whitelist, &mut updated.blacklist, &network_key) {
        return Ok(NetworkTrustLists {
            whitelist: updated.whitelist,
            blacklist: updated.blacklist,
        });
    }
    save_secure_config_verified(&app, &updated)?;
    let lists = NetworkTrustLists {
        whitelist: updated.whitelist.clone(),
        blacklist: updated.blacklist.clone(),
    };
    *state.config.write().unwrap() = updated;
    rust_log(
        &app,
        &state,
        "安全",
        &format!("已移除网络信任记录 {network_key}"),
        "info",
    );
    Ok(lists)
}

async fn portal_logout_attempt(
    method: LoginType,
    account: Option<&Account>,
    compatibility: VpnCompatibility,
    route_context: Option<&PortalRouteContext>,
) -> Result<String, String> {
    let label = method.display_name();
    match tokio::time::timeout(
        std::time::Duration::from_secs(12),
        logout_from_campus_network_rust(
            method,
            account.map(|value| value.user.as_str()),
            account.map(|value| value.pass.as_str()),
            compatibility,
            route_context,
        ),
    )
    .await
    {
        Ok(Ok((true, message))) => Ok(format!("已通过 {label} 完成注销：{message}")),
        Ok(Ok((false, message))) => Err(format!("{label} 拒绝注销：{message}")),
        Ok(Err(error)) => Err(format!("{label} 注销失败：{error}")),
        Err(_) => Err(format!("{label} 注销超过 12 秒")),
    }
}

async fn billing_logout_attempt(
    account: Option<&Account>,
    compatibility: VpnCompatibility,
    current_ip: &str,
) -> Result<String, String> {
    let account = account
        .filter(|value| !value.pass.is_empty())
        .ok_or_else(|| "计费中心注销需要当前账号的已保存密码".to_string())?;
    match tokio::time::timeout(
        std::time::Duration::from_secs(25),
        billing::disconnect_current_session(
            &account.user,
            &account.pass,
            compatibility,
            current_ip,
        ),
    )
    .await
    {
        Ok(Ok(message)) => Ok(format!("已通过计费中心完成注销：{message}")),
        Ok(Err(error)) => Err(error.user_message()),
        Err(_) => Err("计费中心注销超过 25 秒".to_string()),
    }
}

async fn logout_current_session_by_type(
    login_type: &LoginType,
    current_account: Option<&Account>,
    compatibility: VpnCompatibility,
    current_ip: &str,
    route_context: Option<&PortalRouteContext>,
) -> Result<String, String> {
    let mut failures = Vec::new();
    macro_rules! attempt {
        ($future:expr) => {
            match $future.await {
                Ok(message) => return Ok(message),
                Err(error) => failures.push(error),
            }
        };
    }
    match login_type {
        LoginType::Type1 => {
            attempt!(billing_logout_attempt(
                current_account,
                compatibility,
                current_ip
            ));
            attempt!(portal_logout_attempt(
                LoginType::Type3,
                None,
                compatibility,
                route_context
            ));
            attempt!(portal_logout_attempt(
                LoginType::Type1,
                current_account,
                compatibility,
                route_context
            ));
        }
        LoginType::Type2 => {
            attempt!(portal_logout_attempt(
                LoginType::Type3,
                None,
                compatibility,
                route_context
            ));
            attempt!(portal_logout_attempt(
                LoginType::Type2,
                None,
                compatibility,
                route_context
            ));
            attempt!(billing_logout_attempt(
                current_account,
                compatibility,
                current_ip
            ));
        }
        LoginType::Type3 => {
            attempt!(portal_logout_attempt(
                LoginType::Type3,
                None,
                compatibility,
                route_context
            ));
            attempt!(billing_logout_attempt(
                current_account,
                compatibility,
                current_ip
            ));
        }
        LoginType::Unknown => return Err("无法识别当前校园网类型".to_string()),
    }
    Err(format!("所有注销途径均未成功：{}", failures.join("；")))
}

fn emit_session_network_state(
    app: &tauri::AppHandle,
    state: &AppState,
    network: &serde_json::Value,
    state_name: &str,
    login_type: &LoginType,
) {
    let payload = serde_json::json!({
        "state": state_name,
        "loginType": login_type.as_str(),
        "interfaceName": network["interfaceName"],
        "transport": network["transport"],
        "systemOnline": state.system_online.load(Ordering::SeqCst),
        "ssid": network.get("ssid").and_then(|value| value.as_str()).unwrap_or(""),
        "bssid": network.get("bssid").and_then(|value| value.as_str()).unwrap_or(""),
        "ip": network.get("ip").and_then(|value| value.as_str()).unwrap_or(""),
        "timestamp": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    });
    *state.last_network_state.lock().unwrap() = payload.clone();
    let _ = app.emit("network-state-change", payload);
    #[cfg(desktop)]
    refresh_tray_menu(app, state);
}

async fn current_or_detected_login_type(
    state: &AppState,
    compatibility: VpnCompatibility,
    network: &serde_json::Value,
    route_context: Option<&PortalRouteContext>,
) -> LoginType {
    let cached = state
        .last_network_state
        .lock()
        .unwrap()
        .get("loginType")
        .and_then(serde_json::Value::as_str)
        .and_then(login_type_from_profile);
    if let Some(cached) = cached {
        return cached;
    }
    let detection = detect_login_type_details_rust(
        compatibility,
        network
            .get("ssid")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
        network_transport(network),
        route_context,
    )
    .await;
    if detection.login_ready || detection.portal_detected {
        detection.login_type
    } else {
        LoginType::Unknown
    }
}

#[tauri::command]
async fn logout_current_campus_session(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    account_user: Option<String>,
) -> Result<String, String> {
    ensure_billing_foreground(&state)?;
    let config = state.config.read().unwrap().clone();
    let compatibility = effective_vpn_compatibility(&config);
    let network = get_network_info(app.clone(), Some(true));
    #[cfg(target_os = "android")]
    let wifi_route_guard = AndroidWifiRouteGuard::bind_if_required(&network);
    #[cfg(target_os = "android")]
    if wifi_route_guard.failed() {
        return Err("无法将注销请求绑定到当前校园 Wi-Fi".to_string());
    }
    let route_context = portal_route_context_from_network(&network)?;
    let login_type =
        current_or_detected_login_type(&state, compatibility, &network, route_context.as_ref())
            .await;
    if login_type == LoginType::Unknown {
        return Err("未能确认当前校园网类型，未发送注销请求".to_string());
    }
    let current_account = account_user
        .as_deref()
        .and_then(|user| {
            config
                .accounts
                .iter()
                .find(|account| account.user == user)
                .cloned()
        })
        .or_else(|| preferred_billing_account(&config));
    let current_ip = network
        .get("ip")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let message = run_billing_mutation_to_completion(&state, async {
        logout_current_session_by_type(
            &login_type,
            current_account.as_ref(),
            compatibility,
            current_ip,
            route_context.as_ref(),
        )
        .await
    })
    .await?;
    state.billing_sessions.lock().unwrap().clear();
    rust_log(&app, &state, "登录", &message, "success");
    emit_session_network_state(&app, &state, &network, "BjutCampus", &login_type);
    Ok(message)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManualLoginRequest {
    account_index: Option<usize>,
    login_type_override: Option<String>,
    trust_network_once: Option<bool>,
    expected_network_key: Option<String>,
    switch_context: Option<LoginSwitchContext>,
    operation_id: Option<String>,
}

#[tauri::command]
async fn manual_login(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    request: ManualLoginRequest,
) -> Result<ManualLoginResult, String> {
    let id = request
        .operation_id
        .clone()
        .unwrap_or_else(|| format!("login-{}", chrono::Utc::now().timestamp_millis()));
    state.login_progress.start(&id)?;
    let progress = login_progress::Session {
        id: &id,
        control: &state.login_progress,
        app: &app,
    };
    let result = manual_login_inner(app.clone(), state.inner().clone(), request, &progress).await;
    match &result {
        Ok(value) => progress.finish(value.success, &value.message),
        Err(error) => progress.finish(false, error),
    }
    result
}

#[tauri::command]
fn cancel_manual_login(
    state: tauri::State<Arc<AppState>>,
    operation_id: String,
) -> Result<(), String> {
    state.login_progress.cancel(&operation_id)
}

async fn verify_login_network(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    network: &serde_json::Value,
) -> (bool, dual_stack::DualStackReport) {
    let connection = connectivity::observe(app, state, network)
        .wait_for_login()
        .await;
    let online = connection.online();
    let health = if connection.cancelled {
        dual_stack::unavailable("探测期间网络或认证状态改变，请重新检测")
    } else {
        connection
            .health
            .unwrap_or_else(|| dual_stack::unavailable("双栈探测仍在后台继续"))
    };
    (online, health)
}

async fn manual_login_inner(
    app: tauri::AppHandle,
    state: Arc<AppState>,
    request: ManualLoginRequest,
    progress: &login_progress::Session<'_>,
) -> Result<ManualLoginResult, String> {
    let ManualLoginRequest {
        account_index,
        login_type_override,
        trust_network_once,
        expected_network_key,
        switch_context,
        ..
    } = request;
    // Invalidate any countdown-triggered check that captured the previous
    // portal state. This prevents a check started during the logout/login gap
    // from traversing every configured account after the target login wins.
    if state.manual_login_in_progress.swap(true, Ordering::SeqCst) {
        return Ok(ManualLoginResult {
            success: false,
            message: "已有登录或切换账号操作正在进行，请稍候".to_string(),
        });
    }
    state
        .login_operation_generation
        .fetch_add(1, Ordering::SeqCst);
    connectivity::invalidate(&app, &state);
    let _operation_guard = ManualLoginOperationGuard {
        generation: &state.login_operation_generation,
        active: &state.manual_login_in_progress,
    };
    // Capture a complete identity before the potentially slow portal probe.
    // The probe result must never be combined with SSID/BSSID/IP collected
    // from an earlier network.
    progress.phase("checking", "正在核对认证网卡与网络身份")?;
    let network_before_probe = get_network_info(app.clone(), Some(true));
    let identity_before_probe = NetworkIdentitySnapshot::capture(&network_before_probe);
    #[cfg(target_os = "android")]
    let wifi_route_guard = AndroidWifiRouteGuard::bind_if_required(&network_before_probe);
    #[cfg(target_os = "android")]
    if wifi_route_guard.failed() {
        return Ok(ManualLoginResult {
            success: false,
            message: "无法将认证请求绑定到当前校园 Wi-Fi；已停止使用 VPN/移动数据路径".to_string(),
        });
    }
    if is_mobile_data_network(&network_before_probe) {
        rust_log(
            &app,
            &state,
            "登录",
            "当前使用移动数据，已阻止校园网网关探测和手动登录",
            "info",
        );
        return Ok(ManualLoginResult {
            success: false,
            message: "当前使用移动数据，未连接 Wi-Fi；已停止校园网网关探测".to_string(),
        });
    }
    let config = state.config.read().unwrap().clone();
    let compatibility = effective_vpn_compatibility(&config);
    let probe_route_context = portal_route_context_from_network(&network_before_probe)?;
    progress.phase("gateway", "正在确认校园认证网关")?;
    let detection = detect_login_type_details_rust(
        compatibility,
        network_before_probe
            .get("ssid")
            .and_then(|value| value.as_str())
            .unwrap_or(""),
        network_transport(&network_before_probe),
        probe_route_context.as_ref(),
    )
    .await;
    let detected_type = if detection.login_ready {
        detection.login_type.clone()
    } else {
        LoginType::Unknown
    };
    let network = get_network_info(app.clone(), Some(true));
    if let Err(change) = ensure_same_network_identity(&identity_before_probe, &network) {
        let message = format!("协议探测期间检测到网络切换（{change}），已停止发送账号密码");
        rust_log(&app, &state, "安全", &message, "error");
        return Ok(ManualLoginResult {
            success: false,
            message,
        });
    }
    let portal_route_context = portal_route_context_from_network(&network)?;
    let login_type =
        parse_login_type_override(login_type_override.as_deref()).unwrap_or(detected_type);
    if login_type == LoginType::Unknown {
        return Ok(ManualLoginResult {
            success: false,
            message: if type1_portal_requires_maximum(&detection, compatibility) {
                TYPE1_MAXIMUM_MODE_REQUIRED_MESSAGE.to_string()
            } else {
                "未检测到校园网登录页面".to_string()
            },
        });
    }

    let ssid = network
        .get("ssid")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let bssid = network
        .get("bssid")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let ip = network
        .get("ip")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let trust = evaluate_network_trust(NetworkTrustInput {
        login_type: &login_type,
        ssid,
        bssid,
        ip,
        transport: network_transport(&network),
        identity_fresh: network_identity_is_fresh(&network),
        whitelist: &config.whitelist,
        blacklist: &config.blacklist,
    });
    match trust.decision {
        NetworkTrustDecision::Allowed => {}
        NetworkTrustDecision::Blocked => {
            return Ok(ManualLoginResult {
                success: false,
                message: format!("网络安全检查未通过：{}", trust.reason),
            });
        }
        NetworkTrustDecision::NeedsConfirmation if trust_network_once != Some(true) => {
            return Ok(ManualLoginResult {
                success: false,
                message: format!("当前网络需要用户确认：{}", trust.reason),
            });
        }
        NetworkTrustDecision::NeedsConfirmation
            if expected_network_key.as_deref() != Some(trust.network_key.as_str()) =>
        {
            return Ok(ManualLoginResult {
                success: false,
                message: "确认期间网络已经切换，请重新确认当前网络".to_string(),
            });
        }
        NetworkTrustDecision::NeedsConfirmation => {
            rust_log(
                &app,
                &state,
                "安全",
                &format!("用户已确认仅本次信任当前网络：{}", trust.network_key),
                "info",
            );
        }
    }
    let trusted_identity = NetworkIdentitySnapshot::capture(&network);

    let switch_context = switch_context.unwrap_or_default();
    let switching_account = switch_context.enabled;
    if switching_account && account_index.is_none() {
        return Ok(ManualLoginResult {
            success: false,
            message: "切换账号前请明确选择目标账号".to_string(),
        });
    }
    let current_account = switch_context
        .current_account_user
        .as_deref()
        .and_then(|user| {
            config
                .accounts
                .iter()
                .find(|account| account.user == user)
                .cloned()
        })
        .or_else(|| preferred_billing_account(&config));
    let configured_accounts = config.accounts;
    let accounts: Vec<Account> = match account_index {
        Some(index) => configured_accounts
            .get(index)
            .cloned()
            .into_iter()
            .collect(),
        None => configured_accounts,
    }
    .into_iter()
    .filter(|account| account_index.is_some() || !account.is_disabled.unwrap_or(false))
    .collect();
    if accounts.is_empty() {
        return Ok(ManualLoginResult {
            success: false,
            message: "未配置可用账号".to_string(),
        });
    }

    // Serialize the credential-bearing/logout portion with automatic login.
    // Read-only identity/protocol probes above remain concurrent and cheap.
    progress.phase("queued", "等待正在进行的认证操作结束")?;
    let _login_guard = state.login_request_lock.lock().await;
    progress.control.check(progress.id)?;

    if switching_account {
        let target = &accounts[0];
        if target.pass.is_empty() {
            return Ok(ManualLoginResult {
                success: false,
                message: format!("目标账号 {} 缺少已保存的密码", target.user),
            });
        }
        if let Err(remaining) = account_attempt_allowed(&state, &target.user) {
            return Ok(ManualLoginResult {
                success: false,
                message: format!(
                    "目标账号 {} 正在冷却，剩余 {} 秒；可在网络诊断页解除",
                    target.user, remaining
                ),
            });
        }
        if switch_context.current_account_user.as_deref() == Some(target.user.as_str()) {
            return Ok(ManualLoginResult {
                success: true,
                message: format!("当前已经使用账号 {} 登录", target.user),
            });
        }
        if login_type != LoginType::Type2 {
            progress.submitting("logout", "正在注销旧账号，完成后将切换到所选账号")?;
            let logout_message = logout_current_session_by_type(
                &login_type,
                current_account.as_ref(),
                compatibility,
                ip,
                portal_route_context.as_ref(),
            )
            .await
            .map_err(|error| format!("切换账号前注销当前会话失败：{error}"))?;
            rust_log(&app, &state, "登录", &logout_message, "success");
            state.billing_sessions.lock().unwrap().clear();
            emit_session_network_state(&app, &state, &network, "BjutCampus", &login_type);
            // The accounting backend keeps the released IP/session visible
            // briefly after a successful logout. Captured and real-device
            // tests show that submitting the target account immediately can
            // be rejected as "same IP already online". Give all logout paths
            // a fixed two-second convergence window before the next login.
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }

    let mut last_failure_message: Option<String> = None;
    for account in accounts {
        if account.pass.is_empty() {
            last_failure_message = Some(format!("账号 {} 缺少已保存的密码", account.user));
            rust_log(
                &app,
                &state,
                "登录",
                &format!("账号 {} 缺少已保存的密码", account.user),
                "error",
            );
            continue;
        }
        if let Err(remaining) = account_attempt_allowed(&state, &account.user) {
            last_failure_message = Some(format!(
                "账号 {} 正在冷却，剩余 {} 秒；可在网络诊断页解除",
                account.user, remaining
            ));
            rust_log(
                &app,
                &state,
                "账号健康",
                &format!(
                    "账号 {} 正在冷却，剩余 {} 秒；可在网络诊断页解除",
                    account.user, remaining
                ),
                "info",
            );
            continue;
        }
        // A failed account may take long enough for the user to roam or switch
        // interfaces. Re-read the complete identity immediately before every
        // credential submission, not merely before the first account.
        let latest_network = get_network_info(app.clone(), Some(true));
        if !network_identity_is_fresh(&latest_network) {
            let message =
                "发送账号密码前无法确认 SSID/BSSID/IP 来自同一物理接口，已停止登录".to_string();
            rust_log(&app, &state, "安全", &message, "error");
            return Ok(ManualLoginResult {
                success: false,
                message,
            });
        }
        if let Err(change) = ensure_same_network_identity(&trusted_identity, &latest_network) {
            let message = format!("发送账号密码前检测到网络切换（{change}），已停止登录");
            rust_log(&app, &state, "安全", &message, "error");
            return Ok(ManualLoginResult {
                success: false,
                message,
            });
        }
        rust_log(
            &app,
            &state,
            "登录",
            &format!("尝试使用账号 {} 登录...", account.user),
            "info",
        );
        progress.phase("preparing", "正在准备认证连接与地址")?;
        let preparation_refused = AtomicBool::new(false);
        let before_submit = || {
            let latest = get_network_info(app.clone(), Some(true));
            if let Err(error) = ensure_same_network_identity(&trusted_identity, &latest) {
                preparation_refused.store(true, Ordering::SeqCst);
                return Err(error);
            }
            connectivity::invalidate(&app, &state);
            progress.submitting("authenticating", "正在提交认证，随后核对联网结果")
        };
        match portal_auth::login_with_submission_guard(
            login_type.clone(),
            &account.user,
            &account.pass,
            compatibility,
            portal_route_context.as_ref(),
            &before_submit,
        )
        .await
        {
            Ok((true, message)) => {
                record_account_success(&app, &state, &account.user);
                #[cfg(target_os = "android")]
                report_android_campus_wifi_connected(&network);
                rust_log(
                    &app,
                    &state,
                    "登录",
                    &format!("登录成功: {message}"),
                    "success",
                );
                progress.phase("verifying", "认证已接受，正在分别验证 IPv4 与 IPv6 联网")?;
                let mut message = message;
                let net_info = get_network_info(app.clone(), Some(false));
                if ensure_same_network_identity(
                    &trusted_identity,
                    &get_network_info(app.clone(), Some(true)),
                )
                .is_ok()
                {
                    let (verified_online, mut health) =
                        verify_login_network(&app, &state, &net_info).await;
                    if ensure_same_network_identity(
                        &trusted_identity,
                        &get_network_info(app.clone(), Some(true)),
                    )
                    .is_err()
                    {
                        health.invalidate("认证后检测到网络切换，请重新检测");
                        let _ = app.emit("link-health", &health);
                        schedule_network_change_readiness(app.clone(), state.clone());
                        return Ok(ManualLoginResult {
                            success: true,
                            message: format!("{message}；认证已接受，但网络已切换，请重新检测"),
                        });
                    }
                    if !verified_online {
                        message.push_str("；公网连通性尚未通过验证");
                    }
                } else {
                    schedule_network_change_readiness(app.clone(), state.clone());
                    return Ok(ManualLoginResult {
                        success: true,
                        message: format!("{message}；认证后网卡发生变化，请重新检测"),
                    });
                }
                emit_session_network_state(&app, &state, &net_info, "Online", &login_type);
                let _ = app.emit(
                    "dashboard-user-info-refresh",
                    serde_json::json!({
                        "reason": if switching_account { "account-switch" } else { "manual-login" }
                    }),
                );
                return Ok(ManualLoginResult {
                    success: true,
                    message,
                });
            }
            Ok((false, message)) => match classify_portal_login_failure(&message) {
                PortalLoginFailureDisposition::CredentialRejected => {
                    last_failure_message = Some(message.clone());
                    record_account_failure(&app, &state, &account.user, &message);
                    rust_log(
                        &app,
                        &state,
                        "登录",
                        &format!("登录失败: {message}"),
                        "error",
                    );
                }
                PortalLoginFailureDisposition::SessionAlreadyOnline => {
                    progress.phase("verifying", "网关报告已有会话，正在验证互联网连通性")?;
                    if verify_login_network(&app, &state, &network).await.0 {
                        rust_log(
                            &app,
                            &state,
                            "登录",
                            "认证网关报告当前 IP 已在线，且互联网已连通；停止尝试其他账号",
                            "success",
                        );
                        let net_info = get_network_info(app.clone(), Some(true));
                        if ensure_same_network_identity(&trusted_identity, &net_info).is_err() {
                            schedule_network_change_readiness(app.clone(), state.clone());
                            return Ok(ManualLoginResult {
                                success: false,
                                message: "核对已有会话时网卡发生变化，请重新检测".to_string(),
                            });
                        }
                        emit_session_network_state(&app, &state, &net_info, "Online", &login_type);
                        let _ = app.emit(
                            "dashboard-user-info-refresh",
                            serde_json::json!({"reason": "existing-session"}),
                        );
                        return Ok(ManualLoginResult {
                            success: true,
                            message: "当前 IP 已存在可用的校园网在线会话".to_string(),
                        });
                    }
                    rust_log(
                        &app,
                        &state,
                        "登录",
                        "认证网关报告当前 IP 已在线，但互联网尚未验证；已停止尝试其他账号",
                        "info",
                    );
                    return Ok(ManualLoginResult {
                        success: false,
                        message,
                    });
                }
                PortalLoginFailureDisposition::StopTraversal => {
                    rust_log(
                        &app,
                        &state,
                        "登录",
                        &format!("认证网关返回非账号特定错误，已停止尝试其他账号：{message}"),
                        "error",
                    );
                    return Ok(ManualLoginResult {
                        success: false,
                        message,
                    });
                }
            },
            Err(error) => {
                if preparation_refused.load(Ordering::SeqCst)
                    || !progress.control.submitted(progress.id)
                    || progress.control.check(progress.id).is_err()
                {
                    return Err(error);
                }
                progress.phase("verifying", "认证响应未能确认，正在只读核对联网状态")?;
                let _verified = verify_login_network(&app, &state, &network).await;
                if login_result_is_ambiguous(&error) {
                    rust_log(
                        &app,
                        &state,
                        "登录",
                        &format!(
                            "账号 {} 的登录结果无法确认；已停止继续尝试其他账号：{}",
                            account.user, error
                        ),
                        "error",
                    );
                    return Ok(ManualLoginResult {
                        success: false,
                        message: error,
                    });
                }
                rust_log(
                    &app,
                    &state,
                    "登录",
                    &format!("登录请求出现非账号特定错误，已停止尝试其他账号：{error}"),
                    "error",
                );
                return Ok(ManualLoginResult {
                    success: false,
                    message: error,
                });
            }
        }
    }
    Ok(ManualLoginResult {
        success: false,
        message: last_failure_message.unwrap_or_else(|| "所有可用账号均未能登录".to_string()),
    })
}

fn first_decimal(value: &str) -> Option<f64> {
    let number = value
        .chars()
        .filter(|character| character.is_ascii_digit() || *character == '.')
        .collect::<String>();
    number.parse::<f64>().ok()
}

fn evaluate_usage_alerts(app: &tauri::AppHandle, state: &AppState, info: &UserInfo) {
    let (enabled, balance_threshold, flow_threshold) = {
        let config = state.config.read().unwrap();
        (
            config.usage_alerts,
            config.balance_alert_threshold,
            config.flow_alert_threshold,
        )
    };
    if !enabled {
        return;
    }
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut alerts = Vec::new();
    if let Some(balance) = first_decimal(&info.balance) {
        if balance <= balance_threshold {
            alerts.push((
                "balance",
                format!(
                    "校园网余额仅剩 {}，已低于 {:.2} 元提醒线",
                    info.balance, balance_threshold
                ),
            ));
        }
    }
    if info.flow != "无限" {
        if let Some(flow) = first_decimal(&info.flow) {
            if flow <= flow_threshold {
                alerts.push((
                    "flow",
                    format!(
                        "套餐流量仅剩 {}，已低于 {:.2} GB 提醒线",
                        info.flow, flow_threshold
                    ),
                ));
            }
        }
    }
    for (kind, message) in alerts {
        let should_notify = {
            let mut history = state.usage_alert_history.lock().unwrap();
            if history.get(kind) == Some(&today) {
                false
            } else {
                history.insert(kind.to_string(), today.clone());
                true
            }
        };
        if should_notify {
            rust_log(app, state, "用量提醒", &message, "error");
            let _ = show_native_notification(app, "校园网用量提醒", &message);
            let _ = app.emit(
                "usage-alert",
                serde_json::json!({ "kind": kind, "message": message }),
            );
        }
    }
}

fn preferred_billing_account(config: &AppConfig) -> Option<Account> {
    config
        .accounts
        .iter()
        .filter(|account| !account.user.trim().is_empty())
        .find(|account| account.is_default)
        .or_else(|| {
            config
                .accounts
                .iter()
                .find(|account| !account.user.trim().is_empty())
        })
        .cloned()
}

fn selected_billing_account(config: &AppConfig, account_user: Option<&str>) -> Option<Account> {
    let requested = account_user
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match requested {
        Some(user) => config
            .accounts
            .iter()
            .find(|account| account.user == user && !account.user.trim().is_empty())
            .cloned(),
        None => preferred_billing_account(config),
    }
}

#[cfg(desktop)]
fn promote_default_account(accounts: &mut Vec<Account>, index: usize) -> Option<String> {
    if index >= accounts.len() {
        return None;
    }
    let mut preferred = accounts.remove(index);
    preferred.is_default = true;
    let preferred_user = preferred.user.clone();
    for account in accounts.iter_mut() {
        account.is_default = false;
    }
    accounts.insert(0, preferred);
    Some(preferred_user)
}

fn emit_billing_center_progress(
    app: &tauri::AppHandle,
    state: &AppState,
    message: &str,
    percent: u8,
) {
    rust_log(app, state, "计费", message, "info");
    let _ = app.emit(
        "billing-center-progress",
        serde_json::json!({ "message": message, "percent": percent.min(100) }),
    );
}

#[tauri::command]
async fn discover_current_campus_account(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Option<serde_json::Value>, String> {
    *state.pending_discovered_account.lock().await = None;
    if ensure_billing_foreground(&state).is_err() {
        return Ok(None);
    }
    let compatibility = {
        let config = state.config.read().unwrap();
        effective_vpn_compatibility(&config)
    };
    let request = tokio::time::timeout(
        std::time::Duration::from_secs(25),
        billing::discover_current_campus_account(compatibility),
    );
    let background = billing_runtime::wait_for_background(&state.is_in_background);
    futures_util::pin_mut!(request, background);
    let result = match futures_util::future::select(request, background).await {
        futures_util::future::Either::Left((result, _)) => Some(result),
        futures_util::future::Either::Right((_, _)) => None,
    };
    let Some(result) = result else {
        rust_log(
            &app,
            &state,
            "账号",
            "App 已进入后台，已停止校园网账号发现",
            "debug",
        );
        return Ok(None);
    };
    match result {
        Ok(Ok(Some(account))) => {
            let user = account.user.clone();
            let Some(password) = account.pass else {
                rust_log(
                    &app,
                    &state,
                    "账号",
                    if account.password_is_temporary {
                        "已识别当前校园网账号，但 dashboard 仅返回临时密码字段；等待用户自行补录密码"
                    } else {
                        "已识别当前校园网账号，但 dashboard 未提供可保存的密码；等待用户自行补录密码"
                    },
                    "info",
                );
                return Ok(Some(serde_json::json!({
                    "user": user,
                    "passwordRequired": true
                })));
            };
            rust_log(
                &app,
                &state,
                "账号",
                "已识别当前校园网会话中的账号，等待用户确认是否保存",
                "info",
            );
            let token_seed = format!(
                "{}:{}:{:?}",
                user,
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
                std::thread::current().id()
            );
            let token = format!("{:x}", md5::compute(token_seed));
            *state.pending_discovered_account.lock().await = Some(PendingDiscoveredAccount {
                account: Account {
                    user: account.user,
                    pass: password,
                    is_default: false,
                    is_disabled: Some(false),
                },
                token: token.clone(),
                expires_at: std::time::Instant::now() + std::time::Duration::from_secs(120),
            });
            Ok(Some(serde_json::json!({ "user": user, "token": token })))
        }
        Ok(Ok(None)) => Ok(None),
        Ok(Err(billing::BillingError::Network(detail))) => {
            rust_log(
                &app,
                &state,
                "账号",
                &format!("当前网络未提供校园网账号发现入口：{detail}"),
                "debug",
            );
            Ok(None)
        }
        Ok(Err(error)) => {
            let message = error.user_message();
            rust_log(&app, &state, "账号", &message, "debug");
            Err(message)
        }
        Err(_) => {
            rust_log(&app, &state, "账号", "校园网账号发现超时", "debug");
            Ok(None)
        }
    }
}

#[tauri::command]
async fn accept_discovered_campus_account(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    user: String,
    token: String,
) -> Result<(), String> {
    let pending = state
        .pending_discovered_account
        .lock()
        .await
        .take()
        .filter(|pending| {
            pending.account.user == user
                && pending.token == token
                && std::time::Instant::now() <= pending.expires_at
        })
        .ok_or_else(|| "待保存的校园网账号凭据已失效，请重新检测".to_string())?;
    let discovered = pending.account;
    let mut config = state.config.read().unwrap().clone();
    if config
        .accounts
        .iter()
        .any(|account| account.user == discovered.user)
    {
        return Ok(());
    }
    let mut discovered = discovered;
    discovered.is_default = config.accounts.is_empty();
    config.accounts.push(discovered);
    save_config(&app, &state, config)?;
    rust_log(
        &app,
        &state,
        "账号",
        "已将发现的校园网账号直接写入安全存储",
        "success",
    );
    Ok(())
}

#[tauri::command]
async fn reject_discovered_campus_account(
    state: tauri::State<'_, Arc<AppState>>,
    token: String,
) -> Result<(), String> {
    let mut pending = state.pending_discovered_account.lock().await;
    if pending
        .as_ref()
        .is_some_and(|candidate| candidate.token == token)
    {
        *pending = None;
    }
    Ok(())
}

#[tauri::command]
async fn get_user_info(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    local_ip: Option<String>,
    force: Option<bool>,
) -> Result<Option<UserInfo>, String> {
    let compatibility = {
        let config = state.config.read().unwrap();
        effective_vpn_compatibility(&config)
    };
    let _ = force;
    let network = get_network_info(app.clone(), Some(false));
    #[cfg(target_os = "android")]
    let wifi_route_guard = AndroidWifiRouteGuard::bind_if_required(&network);
    #[cfg(target_os = "android")]
    if wifi_route_guard.failed() {
        return Err("无法将用户信息请求绑定到当前校园 Wi-Fi".to_string());
    }
    let portal_route_context = portal_route_context_from_network(&network)?;
    let effective_ip = network
        .get("ip")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .or(local_ip.as_deref());
    let info =
        fetch_portal_user_info(effective_ip, compatibility, portal_route_context.as_ref()).await;
    if let Some(info) = info.as_ref() {
        evaluate_usage_alerts(&app, &state, info);
    }
    Ok(info)
}

#[tauri::command]
async fn get_remaining_flow(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Option<String>, String> {
    ensure_billing_foreground(&state)?;
    let compatibility = {
        let config = state.config.read().unwrap();
        effective_vpn_compatibility(&config)
    };
    let network = get_network_info(app, Some(false));
    #[cfg(target_os = "android")]
    let wifi_route_guard = AndroidWifiRouteGuard::bind_if_required(&network);
    #[cfg(target_os = "android")]
    if wifi_route_guard.failed() {
        return Err("无法将剩余流量请求绑定到当前校园 Wi-Fi".to_string());
    }
    let route_context = portal_route_context_from_network(&network)?;
    match tokio::time::timeout(
        std::time::Duration::from_secs(12),
        billing::fetch_current_remaining_flow_via_lgn(compatibility, route_context.as_ref()),
    )
    .await
    {
        Ok(Ok(flow)) => Ok(flow),
        Ok(Err(error)) => Err(error.user_message()),
        Err(_) => Err("剩余流量读取超过 12 秒".to_string()),
    }
}

#[tauri::command]
async fn get_billing_center(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    account_user: Option<String>,
    current_session: Option<bool>,
) -> Result<billing::BillingCenterData, String> {
    emit_billing_center_progress(&app, &state, "准备读取计费中心完整数据", 2);
    let _fetch_guard = match state.billing_fetch_lock.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            emit_billing_center_progress(&app, &state, "正在等待已有的计费刷新完成", 3);
            match tokio::time::timeout(
                std::time::Duration::from_secs(50),
                state.billing_fetch_lock.lock(),
            )
            .await
            {
                Ok(guard) => guard,
                Err(_) => {
                    let error = "等待已有计费刷新超过 50 秒，请稍后重试".to_string();
                    rust_log(&app, &state, "计费", &error, "error");
                    return Err(error);
                }
            }
        }
    };
    let (access, compatibility) = resolve_billing_access(
        &app,
        &state,
        account_user.as_deref(),
        current_session.unwrap_or(false),
        true,
    )
    .await?;
    let progress_app = app.clone();
    let progress_state = state.inner().clone();
    let emit_progress = move |message: &str, percent: u8| {
        emit_billing_center_progress(&progress_app, &progress_state, message, percent);
    };
    let module_app = app.clone();
    let module_account = if current_session.unwrap_or(false) {
        "__current_session__".to_string()
    } else {
        access.account.clone()
    };
    let publish_module = move |update: billing::BillingCenterModuleUpdate| {
        let mut payload = serde_json::to_value(update).unwrap_or_default();
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "account".to_string(),
                serde_json::Value::String(module_account.clone()),
            );
        }
        let _ = module_app.emit("billing-center-module", payload);
    };
    ensure_billing_foreground(&state)?;
    let result = run_billing_read_while_foreground(&state, async {
        match tokio::time::timeout(
            std::time::Duration::from_secs(75),
            billing::fetch_center(
                &access,
                compatibility,
                &state.billing_sessions,
                emit_progress,
                publish_module,
            ),
        )
        .await
        {
            Ok(result) => result.map_err(|error| error.user_message()),
            Err(_) => {
                Err("计费中心完整数据读取超过 75 秒，已停止本次请求；请检查 VPN 后重试".to_string())
            }
        }
    })
    .await;
    match &result {
        Ok(data) => rust_log(
            &app,
            &state,
            "计费",
            &format!(
                "计费中心完整数据已更新（{} 条警告）",
                data.warnings.len() + data.overview.warnings.len()
            ),
            "debug",
        ),
        Err(error) => rust_log(&app, &state, "计费", error, "error"),
    }
    result
}

#[tauri::command]
async fn query_billing_records(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    query: billing::BillingRecordQuery,
    account_user: Option<String>,
    current_session: Option<bool>,
) -> Result<billing::BillingRecordResult, String> {
    let kind = query.kind.clone();
    let page = query.page;
    let all = query.all;
    let _fetch_guard = state.billing_fetch_lock.lock().await;
    let (access, compatibility) = resolve_billing_access(
        &app,
        &state,
        account_user.as_deref(),
        current_session.unwrap_or(false),
        false,
    )
    .await?;
    ensure_billing_foreground(&state)?;
    let result = run_billing_read_while_foreground(&state, async {
        billing::query_records(&access, compatibility, &state.billing_sessions, &query)
            .await
            .map_err(|error| error.user_message())
    })
    .await;
    match &result {
        Ok(data) => rust_log(
            &app,
            &state,
            "计费",
            &format!(
                "计费记录已读取（类型={kind}，页码={}，返回={}，总数={}）",
                if all { 0 } else { page },
                data.table.rows.len(),
                data.table.total
            ),
            "debug",
        ),
        Err(error) => rust_log(&app, &state, "计费", error, "error"),
    }
    result
}

#[tauri::command]
async fn perform_billing_action(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    request: billing::BillingActionRequest,
    account_user: Option<String>,
    current_session: Option<bool>,
) -> Result<billing::BillingActionResult, String> {
    let action = request.action.clone();
    let _billing_guard = if action == "changePassword" {
        None
    } else {
        Some(state.billing_fetch_lock.lock().await)
    };
    let (account, access, compatibility) = if action == "changePassword" {
        (
            campus_service_target(&state, account_user.as_deref())?,
            None,
            None,
        )
    } else {
        let (access, compatibility) = resolve_billing_access(
            &app,
            &state,
            account_user.as_deref(),
            current_session.unwrap_or(false),
            false,
        )
        .await?;
        let account = Account {
            user: access.account.clone(),
            pass: String::new(),
            is_default: false,
            is_disabled: None,
        };
        (account, Some(access), Some(compatibility))
    };
    // A write may succeed even when its response is lost. Never retain a
    // pre-write dashboard session across any mutation attempt.
    state.billing_sessions.lock().unwrap().remove(&account.user);
    let new_password = request.new_password.clone();
    let mut result = if action == "changePassword" {
        ensure_billing_foreground(&state)?;
        let supplied_password = request
            .old_password
            .as_deref()
            .ok_or_else(|| "请输入当前统一认证密码".to_string())?;
        let replacement = request
            .new_password
            .as_deref()
            .ok_or_else(|| "请输入新统一认证密码".to_string())?;
        let _campus_guard = state.campus_service_lock.lock().await;
        ensure_billing_foreground(&state)?;
        rust_log(&app, &state, "计费", "正在通过统一认证修改密码", "info");
        let changed = campus_services::change_password(
            &account.user,
            &account.pass,
            supplied_password,
            replacement,
        )
        .await
        .map_err(campus_services::CampusServiceError::user_message);
        if let Err(error) = &changed {
            rust_log(&app, &state, "计费", error, "error");
        }
        let message = changed?;
        billing::BillingActionResult {
            message,
            password_changed: true,
        }
    } else {
        let compatibility = compatibility.ok_or_else(|| "计费操作缺少 VPN 兼容配置".to_string())?;
        ensure_billing_foreground(&state)?;
        run_billing_mutation_to_completion(&state, async {
            billing::perform_action(
                access.as_ref().ok_or("缺少计费会话")?,
                compatibility,
                &request,
            )
            .await
            .map_err(|error| error.user_message())
        })
        .await?
    };

    if result.password_changed {
        let replacement = new_password
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "统一认证密码已修改，但新密码没有返回到安全存储流程".to_string())?;
        let mut config = state.config.read().unwrap().clone();
        let saved = config
            .accounts
            .iter_mut()
            .find(|saved| saved.user == account.user)
            .ok_or_else(|| {
                "统一认证密码已修改，但 App 中已找不到对应账号，请立即重新添加账号".to_string()
            })?;
        saved.pass = replacement;
        save_config(&app, &state, config).map_err(|error| {
            format!(
                "统一认证密码已修改，但 App 安全存储同步失败：{error}。请立即在账号管理中更新密码"
            )
        })?;
        result.message = format!("{}；App 中的账号密码已同步更新", result.message);
    }
    let action_label = match action.as_str() {
        "stopNow" => "立即停机",
        "reopenNow" => "立即复通",
        "schedulePackage" => "预约套餐",
        "cancelPackage" => "取消套餐预约",
        "setConsumeLimit" => "修改消费保护",
        "bindMac" => "绑定设备",
        "unbindMac" => "解绑设备",
        "changePassword" => "修改统一认证密码",
        "updateQuestions" => "修改密码保护",
        _ => "计费操作",
    };
    rust_log(
        &app,
        &state,
        "计费",
        &format!("用户已确认执行：{action_label}"),
        "success",
    );
    Ok(result)
}

fn campus_service_target(state: &AppState, account_user: Option<&str>) -> Result<Account, String> {
    #[cfg(target_os = "android")]
    clear_android_network_binding()?;
    let config = state.config.read().unwrap();
    let account = selected_billing_account(&config, account_user).ok_or_else(|| {
        if account_user.is_some() {
            "所选统一认证账号不存在或缺少有效配置".to_string()
        } else {
            "没有可用于统一认证的已保存账号".to_string()
        }
    })?;
    if account.pass.is_empty() {
        return Err("统一认证账号缺少已保存的密码".to_string());
    }
    Ok(account)
}

fn campus_service_session_seed(
    state: &AppState,
    account: &str,
) -> Option<campus_services::PersistedCampusSession> {
    state
        .config
        .read()
        .unwrap()
        .campus_service_sessions
        .iter()
        .find(|session| session.account() == account)
        .cloned()
}

fn persist_campus_service_session(
    app: &tauri::AppHandle,
    state: &AppState,
    session: campus_services::PersistedCampusSession,
) {
    let snapshot = {
        let mut config = state.config.write().unwrap();
        config
            .campus_service_sessions
            .retain(|current| current.account() != session.account());
        config.campus_service_sessions.push(session);
        config.clone()
    };
    if let Err(error) = save_secure_config_verified(app, &snapshot) {
        rust_log(
            app,
            state,
            "计费",
            &format!("移动门户登录状态未能写入安全存储：{error}"),
            "error",
        );
    } else {
        rust_log(
            app,
            state,
            "计费",
            "已在安全存储中更新移动门户长期登录状态",
            "debug",
        );
    }
}

fn persist_recharge_transaction(
    app: &tauri::AppHandle,
    state: &AppState,
    transaction: recharge_state::RechargeTransaction,
) -> Result<(), String> {
    let snapshot = {
        let mut config = state.config.write().unwrap();
        config.recharge_transactions.upsert(transaction);
        config.clone()
    };
    save_recharge_snapshot_verified(app, &snapshot)
}

fn save_recharge_snapshot_verified(
    app: &tauri::AppHandle,
    snapshot: &AppConfig,
) -> Result<(), String> {
    let mut last_error = String::new();
    for attempt in 0..3 {
        match save_secure_config_verified(app, snapshot) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = error,
        }
        if attempt < 2 {
            std::thread::sleep(std::time::Duration::from_millis(40 * (attempt + 1)));
        }
    }
    Err(format!("充值恢复记录连续写入失败：{last_error}"))
}

fn transition_recharge_transaction(
    app: &tauri::AppHandle,
    state: &AppState,
    id: &str,
    stage: recharge_state::RechargeStage,
    note: impl Into<String>,
) -> Result<(), String> {
    let snapshot = {
        let mut config = state.config.write().unwrap();
        config
            .recharge_transactions
            .transition(id, stage, chrono::Utc::now().timestamp(), note)?;
        config.clone()
    };
    save_recharge_snapshot_verified(app, &snapshot)
}

fn transition_recharge_transaction_with_parent(
    app: &tauri::AppHandle,
    state: &AppState,
    id: &str,
    stage: recharge_state::RechargeStage,
    note: impl Into<String>,
) -> Result<(), String> {
    let snapshot = {
        let mut config = state.config.write().unwrap();
        config.recharge_transactions.transition_with_parent(
            id,
            stage,
            chrono::Utc::now().timestamp(),
            note,
        )?;
        config.clone()
    };
    save_recharge_snapshot_verified(app, &snapshot)
}

#[tauri::command]
fn get_recoverable_recharges(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
) -> Vec<recharge_state::RechargeRecoveryView> {
    let (views, reconciled_snapshot) = {
        let mut config = state.config.write().unwrap();
        let changed = config
            .recharge_transactions
            .reconcile_legacy_completed_transfers();
        let expired = config
            .recharge_transactions
            .prune_expired(chrono::Utc::now().timestamp());
        let views = config.recharge_transactions.recovery_views();
        (views, (changed || expired).then(|| config.clone()))
    };
    if let Some(snapshot) = reconciled_snapshot {
        if let Err(error) = save_recharge_snapshot_verified(&app, &snapshot) {
            rust_log(
                &app,
                &state,
                "计费",
                &format!("旧版充值恢复状态已在内存修复，但持久化失败：{error}"),
                "error",
            );
        }
    }
    views
}

#[tauri::command]
fn finish_recharge_recovery(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    id: String,
    completed: bool,
    note: Option<String>,
) -> Result<(), String> {
    transition_recharge_transaction(
        &app,
        &state,
        &id,
        if completed {
            recharge_state::RechargeStage::Completed
        } else {
            recharge_state::RechargeStage::Unknown
        },
        note.unwrap_or_else(|| {
            if completed {
                "充值流程已完成".to_string()
            } else {
                "充值结果仍需核对".to_string()
            }
        }),
    )
}

fn campus_recharge_open_at_hour(hour: u64) -> bool {
    (6..23).contains(&hour)
}

fn ensure_campus_recharge_open() -> Result<(), String> {
    let unix_seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let beijing_hour = (unix_seconds / 3600 + 8) % 24;
    if campus_recharge_open_at_hour(beijing_hour) {
        Ok(())
    } else {
        Err(
            "充值系统仅在北京时间每日 06:00–23:00 开放；当前可查看余额，但不能创建或确认充值订单"
                .to_string(),
        )
    }
}

#[tauri::command]
async fn prepare_network_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    target_account: String,
    amount: String,
    account_user: Option<String>,
    recovery_source_id: Option<String>,
) -> Result<campus_services::RechargePreview, String> {
    ensure_campus_recharge_open()?;
    let account = campus_service_target(&state, account_user.as_deref())?;
    let _service_guard = state.campus_service_lock.lock().await;
    let session_seed = campus_service_session_seed(&state, &account.user);
    rust_log(
        &app,
        &state,
        "计费",
        "正在通过统一认证核对校园卡与目标网费账户",
        "info",
    );
    #[cfg(target_os = "android")]
    {
        let (transport, validated) = {
            let network = state.last_network_state.lock().unwrap();
            (
                network
                    .get("transport")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
                network
                    .get("validated")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
            )
        };
        rust_log(
            &app,
            &state,
            "计费",
            &format!("统一认证网络上下文: transport={transport}, systemValidated={validated}"),
            "debug",
        );
    }
    let prepared = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        campus_services::prepare_recharge(
            &account.user,
            &account.pass,
            &target_account,
            &amount,
            session_seed,
        ),
    )
    .await
    .map_err(|_| "充值信息核对超过 60 秒，已取消本次请求".to_string())?
    .map_err(campus_services::CampusServiceError::user_message);
    match prepared {
        Ok((pending, preview, persisted)) => {
            persist_campus_service_session(&app, &state, persisted);
            let mut transaction = recharge_state::RechargeTransaction::prepared(
                preview.confirmation_id.clone(),
                "campusCard",
                preview.payer_account.clone(),
                preview.target_account.clone(),
                preview.amount.clone(),
                preview.card_balance.clone(),
                chrono::Utc::now().timestamp(),
            );
            let snapshot = {
                let mut config = state.config.write().unwrap();
                if let Some(source_id) = recovery_source_id.as_deref() {
                    let source = config
                        .recharge_transactions
                        .find(source_id)
                        .cloned()
                        .ok_or_else(|| {
                            "找不到对应的支付恢复记录，请重新打开充值页面".to_string()
                        })?;
                    let source_amount = source.amount.parse::<f64>().unwrap_or(f64::NAN);
                    let transfer_amount = preview.amount.parse::<f64>().unwrap_or(f64::NAN);
                    if !matches!(source.method.as_str(), "alipay" | "wechat")
                        || !source.stage.is_recoverable()
                        || source.payer_account != preview.payer_account
                        || source.target_account != preview.target_account
                        || !source_amount.is_finite()
                        || !transfer_amount.is_finite()
                        || (source_amount - transfer_amount).abs() >= 0.005
                    {
                        return Err("支付恢复记录与本次网费转入信息不一致，已停止操作".to_string());
                    }
                    // Store the journal's stable transaction id instead of a
                    // provider payment id, which is deliberately cleared once
                    // the payment reaches a terminal stage.
                    transaction.parent_id = source.id.clone();
                    config.recharge_transactions.transition(
                        source_id,
                        recharge_state::RechargeStage::PaymentConfirmed,
                        chrono::Utc::now().timestamp(),
                        "已确认支付到账，等待转入目标网费账户",
                    )?;
                }
                config.recharge_transactions.upsert(transaction);
                config.clone()
            };
            save_recharge_snapshot_verified(&app, &snapshot)?;
            *state.campus_recharge_pending.lock().await = Some(pending);
            rust_log(
                &app,
                &state,
                "计费",
                "校园卡与目标网费账户核对完成，等待用户确认",
                "debug",
            );
            Ok(preview)
        }
        Err(error) => {
            *state.campus_recharge_pending.lock().await = None;
            rust_log(&app, &state, "计费", &error, "error");
            Err(error)
        }
    }
}

#[tauri::command]
async fn confirm_network_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    confirmation_id: String,
) -> Result<campus_services::RechargeResult, String> {
    ensure_campus_recharge_open()?;
    ensure_billing_foreground(&state)?;
    let _service_guard = state.campus_service_lock.lock().await;
    ensure_billing_foreground(&state)?;
    let pending = {
        let mut slot = state.campus_recharge_pending.lock().await;
        let current = slot
            .as_ref()
            .ok_or_else(|| "没有待确认的充值，请先重新核对账户与金额".to_string())?;
        if current.confirmation_id() != confirmation_id {
            return Err("充值确认标识不匹配，请重新核对账户与金额".to_string());
        }
        if current.expired() {
            *slot = None;
            return Err("充值确认已过期，请重新核对账户与金额".to_string());
        }
        slot.take()
            .ok_or_else(|| "待确认的充值状态已失效，请重新核对账户与金额".to_string())?
    };
    rust_log(
        &app,
        &state,
        "计费",
        "用户已二次确认校园卡网费充值，正在提交一次性订单",
        "info",
    );
    transition_recharge_transaction_with_parent(
        &app,
        &state,
        &confirmation_id,
        recharge_state::RechargeStage::TransferSubmitted,
        "已提交校园卡到网费账户的写操作",
    )?;
    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(45),
        campus_services::execute_recharge(pending),
    )
    .await
    {
        Ok(result) => result.map_err(campus_services::CampusServiceError::user_message),
        Err(_) => {
            let message =
                "充值提交超过 45 秒，结果未知；请先查询校园卡和网费记录，不要立即重复充值"
                    .to_string();
            let _ = transition_recharge_transaction_with_parent(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Unknown,
                &message,
            );
            return Err(message);
        }
    };
    match &result {
        Ok(_) => {
            let _ = transition_recharge_transaction_with_parent(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Completed,
                "校园卡网费充值成功",
            );
            rust_log(&app, &state, "计费", "校园卡网费充值成功", "success")
        }
        Err(error) => {
            let _ = transition_recharge_transaction_with_parent(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Unknown,
                error,
            );
            rust_log(&app, &state, "计费", error, "error")
        }
    }
    result
}

#[tauri::command]
async fn get_network_recharge_balances(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    target_account: String,
    account_user: Option<String>,
) -> Result<campus_services::RechargeBalanceSnapshot, String> {
    let account = campus_service_target(&state, account_user.as_deref())?;
    let _service_guard = state.campus_service_lock.lock().await;
    let session_seed = campus_service_session_seed(&state, &account.user);
    rust_log(
        &app,
        &state,
        "计费",
        "正在刷新校园卡余额与目标网费余额",
        "debug",
    );
    let snapshot = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        campus_services::query_recharge_balances(
            &account.user,
            &account.pass,
            &target_account,
            session_seed,
        ),
    )
    .await
    .map_err(|_| "余额刷新超过 60 秒，已停止等待".to_string())?
    .map_err(campus_services::CampusServiceError::user_message);
    match snapshot {
        Ok((snapshot, persisted)) => {
            persist_campus_service_session(&app, &state, persisted);
            rust_log(&app, &state, "计费", "充值后余额刷新完成", "success");
            Ok(snapshot)
        }
        Err(error) => {
            rust_log(&app, &state, "计费", &error, "error");
            Err(error)
        }
    }
}

#[tauri::command]
async fn cancel_network_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    confirmation_id: String,
) -> Result<(), String> {
    let mut slot = state.campus_recharge_pending.lock().await;
    if slot
        .as_ref()
        .is_some_and(|pending| pending.confirmation_id() == confirmation_id)
    {
        *slot = None;
        let _ = transition_recharge_transaction(
            &app,
            &state,
            &confirmation_id,
            recharge_state::RechargeStage::Cancelled,
            "用户在提交前取消",
        );
    }
    Ok(())
}

#[tauri::command]
async fn prepare_alipay_card_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    target_account: String,
    amount: String,
    account_user: Option<String>,
) -> Result<campus_services::AlipayRechargePreview, String> {
    ensure_campus_recharge_open()?;
    let account = campus_service_target(&state, account_user.as_deref())?;
    let _service_guard = state.campus_service_lock.lock().await;
    let session_seed = campus_service_session_seed(&state, &account.user);
    rust_log(
        &app,
        &state,
        "计费",
        "正在通过统一认证核对支付宝充值所用校园卡",
        "info",
    );
    let prepared = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        campus_services::prepare_alipay_card_recharge(
            &account.user,
            &account.pass,
            &amount,
            session_seed,
        ),
    )
    .await
    .map_err(|_| "支付宝充值信息核对超过 60 秒，已取消本次请求".to_string())?
    .map_err(campus_services::CampusServiceError::user_message);
    match prepared {
        Ok((pending, preview, persisted)) => {
            persist_campus_service_session(&app, &state, persisted);
            persist_recharge_transaction(
                &app,
                &state,
                recharge_state::RechargeTransaction::prepared(
                    preview.confirmation_id.clone(),
                    "alipay",
                    preview.payer_account.clone(),
                    target_account.clone(),
                    preview.amount.clone(),
                    preview.card_balance.clone(),
                    chrono::Utc::now().timestamp(),
                ),
            )?;
            *state.campus_alipay_recharge_pending.lock().await = Some(pending);
            rust_log(
                &app,
                &state,
                "计费",
                "支付宝充值校园卡信息核对完成，等待用户确认",
                "debug",
            );
            Ok(preview)
        }
        Err(error) => {
            *state.campus_alipay_recharge_pending.lock().await = None;
            rust_log(&app, &state, "计费", &error, "error");
            Err(error)
        }
    }
}

#[tauri::command]
async fn confirm_alipay_card_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    confirmation_id: String,
) -> Result<campus_services::AlipayRechargeResult, String> {
    ensure_campus_recharge_open()?;
    ensure_billing_foreground(&state)?;
    let _service_guard = state.campus_service_lock.lock().await;
    ensure_billing_foreground(&state)?;
    let pending = {
        let mut slot = state.campus_alipay_recharge_pending.lock().await;
        let current = slot
            .as_ref()
            .ok_or_else(|| "没有待确认的支付宝充值，请先重新核对校园卡与金额".to_string())?;
        if current.confirmation_id() != confirmation_id {
            return Err("支付宝充值确认标识不匹配，请重新核对校园卡与金额".to_string());
        }
        if current.expired() {
            *slot = None;
            return Err("支付宝充值确认已过期，请重新核对校园卡与金额".to_string());
        }
        slot.take()
            .ok_or_else(|| "待确认的支付宝充值状态已失效，请重新核对".to_string())?
    };
    rust_log(
        &app,
        &state,
        "计费",
        "用户已二次确认支付宝充值校园卡，正在创建一次性支付订单",
        "info",
    );
    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(45),
        campus_services::execute_alipay_card_recharge(pending),
    )
    .await
    {
        Ok(result) => result.map_err(campus_services::CampusServiceError::user_message),
        Err(_) => {
            let message =
                "支付宝订单创建超过 45 秒，结果未知；请先检查校园卡充值记录，不要立即重复操作"
                    .to_string();
            let _ = transition_recharge_transaction(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Unknown,
                &message,
            );
            return Err(message);
        }
    };
    match &result {
        Ok(payment) => {
            let snapshot = {
                let mut config = state.config.write().unwrap();
                if let Some(transaction) = config
                    .recharge_transactions
                    .0
                    .iter_mut()
                    .find(|item| item.id == confirmation_id)
                {
                    transaction.stage = recharge_state::RechargeStage::HandedOff;
                    transaction.payment_url = payment.payment_url.clone();
                    transaction.updated_at = chrono::Utc::now().timestamp();
                    transaction.note = "支付宝订单已创建，等待付款".to_string();
                }
                config.clone()
            };
            if let Err(error) = save_recharge_snapshot_verified(&app, &snapshot) {
                let message = format!(
                    "支付宝订单可能已经创建，但恢复记录未能安全保存（{error}）。本次不会打开支付入口；请勿重复创建订单，先刷新充值恢复状态"
                );
                rust_log(&app, &state, "计费", &message, "error");
                return Err(message);
            }
            rust_log(&app, &state, "计费", "支付宝支付入口已安全生成", "success")
        }
        Err(error) => {
            let _ = transition_recharge_transaction(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Unknown,
                error,
            );
            rust_log(&app, &state, "计费", error, "error")
        }
    }
    result
}

#[tauri::command]
async fn cancel_alipay_card_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    confirmation_id: String,
) -> Result<(), String> {
    let mut slot = state.campus_alipay_recharge_pending.lock().await;
    if slot
        .as_ref()
        .is_some_and(|pending| pending.confirmation_id() == confirmation_id)
    {
        *slot = None;
        let _ = transition_recharge_transaction(
            &app,
            &state,
            &confirmation_id,
            recharge_state::RechargeStage::Cancelled,
            "用户在创建支付宝订单前取消",
        );
    }
    Ok(())
}

#[tauri::command]
async fn prepare_wechat_card_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    target_account: String,
    amount: String,
    account_user: Option<String>,
) -> Result<campus_services::WechatRechargePreview, String> {
    ensure_campus_recharge_open()?;
    let account = campus_service_target(&state, account_user.as_deref())?;
    let _service_guard = state.campus_service_lock.lock().await;
    let session_seed = campus_service_session_seed(&state, &account.user);
    rust_log(&app, &state, "计费", "正在核对微信充值所用校园卡", "info");
    let prepared = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        campus_services::prepare_wechat_card_recharge(
            &account.user,
            &account.pass,
            &target_account,
            &amount,
            session_seed,
        ),
    )
    .await
    .map_err(|_| "微信充值信息核对超过 60 秒，已取消本次请求".to_string())?
    .map_err(campus_services::CampusServiceError::user_message);
    match prepared {
        Ok((pending, preview, persisted)) => {
            persist_campus_service_session(&app, &state, persisted);
            persist_recharge_transaction(
                &app,
                &state,
                recharge_state::RechargeTransaction::prepared(
                    preview.confirmation_id.clone(),
                    "wechat",
                    preview.payer_account.clone(),
                    preview.target_account.clone(),
                    preview.amount.clone(),
                    preview.card_balance.clone(),
                    chrono::Utc::now().timestamp(),
                ),
            )?;
            *state.campus_wechat_recharge_pending.lock().await = Some(pending);
            rust_log(
                &app,
                &state,
                "计费",
                "微信充值校园卡信息核对完成，等待用户确认",
                "debug",
            );
            Ok(preview)
        }
        Err(error) => {
            *state.campus_wechat_recharge_pending.lock().await = None;
            rust_log(&app, &state, "计费", &error, "error");
            Err(error)
        }
    }
}

#[tauri::command]
async fn confirm_wechat_card_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    confirmation_id: String,
) -> Result<campus_services::WechatRechargeResult, String> {
    ensure_campus_recharge_open()?;
    ensure_billing_foreground(&state)?;
    let _service_guard = state.campus_service_lock.lock().await;
    ensure_billing_foreground(&state)?;
    let pending = {
        let mut slot = state.campus_wechat_recharge_pending.lock().await;
        let current = slot
            .as_ref()
            .ok_or_else(|| "没有待确认的微信充值，请先重新核对校园卡与金额".to_string())?;
        if current.confirmation_id() != confirmation_id {
            return Err("微信充值确认标识不匹配，请重新核对校园卡与金额".to_string());
        }
        if current.expired() {
            *slot = None;
            return Err("微信充值确认已过期，请重新核对校园卡与金额".to_string());
        }
        slot.take()
            .ok_or_else(|| "待确认的微信充值状态已失效，请重新核对".to_string())?
    };
    rust_log(
        &app,
        &state,
        "计费",
        "用户已确认微信充值校园卡，正在创建一次性支付订单",
        "info",
    );
    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(45),
        campus_services::execute_wechat_card_recharge(pending),
    )
    .await
    {
        Ok(result) => result.map_err(campus_services::CampusServiceError::user_message),
        Err(_) => {
            let message =
                "微信订单创建超过 45 秒，结果未知；请先检查校园卡充值记录，不要立即重复操作"
                    .to_string();
            let _ = transition_recharge_transaction(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Unknown,
                &message,
            );
            return Err(message);
        }
    };
    match result {
        Ok((payment, result)) => {
            let (payer, amount, openid, partner_jour_no) = payment.recovery_fields();
            let snapshot = {
                let mut config = state.config.write().unwrap();
                if let Some(transaction) = config
                    .recharge_transactions
                    .0
                    .iter_mut()
                    .find(|item| item.id == confirmation_id)
                {
                    transaction.stage = recharge_state::RechargeStage::HandedOff;
                    transaction.payment_id = result.payment_id.clone();
                    transaction.payment_url = result.launch_url.clone();
                    transaction.payer_account = payer.to_string();
                    transaction.amount = amount.to_string();
                    transaction.openid = openid.to_string();
                    transaction.partner_jour_no = partner_jour_no.to_string();
                    transaction.updated_at = chrono::Utc::now().timestamp();
                    transaction.note = "微信订单已创建，等待付款".to_string();
                }
                config.clone()
            };
            *state.campus_wechat_payment_pending.lock().await = Some(payment);
            if let Err(error) = save_recharge_snapshot_verified(&app, &snapshot) {
                let message = format!(
                    "微信订单可能已经创建，但恢复记录未能安全保存（{error}）。本次不会唤起微信；请勿重复创建订单，先刷新充值恢复状态"
                );
                rust_log(&app, &state, "计费", &message, "error");
                return Err(message);
            }
            rust_log(
                &app,
                &state,
                "计费",
                "Tenpay 会话已续接并取得受信任的微信唤起地址",
                "success",
            );
            Ok(result)
        }
        Err(error) => {
            let _ = transition_recharge_transaction(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Unknown,
                &error,
            );
            rust_log(&app, &state, "计费", &error, "error");
            Err(error)
        }
    }
}

#[tauri::command]
async fn check_wechat_card_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    payment_id: String,
) -> Result<campus_services::WechatPaymentStatus, String> {
    let _service_guard = state.campus_service_lock.lock().await;
    let needs_restore = {
        let slot = state.campus_wechat_payment_pending.lock().await;
        slot.as_ref()
            .is_none_or(|pending| pending.payment_id() != payment_id || pending.expired())
    };
    if needs_restore {
        let transaction = state
            .config
            .read()
            .unwrap()
            .recharge_transactions
            .find(&payment_id)
            .cloned()
            .ok_or_else(|| "没有可恢复的微信支付订单".to_string())?;
        if transaction.method != "wechat" || !transaction.stage.is_recoverable() {
            return Err("微信支付订单已结束或不可恢复".to_string());
        }
        let account = campus_service_target(&state, Some(&transaction.payer_account))?;
        let session_seed = campus_service_session_seed(&state, &account.user);
        let (pending, persisted) = campus_services::restore_wechat_card_recharge(
            &transaction.payment_id,
            &account.user,
            &account.pass,
            &transaction.amount,
            &transaction.partner_jour_no,
            session_seed,
        )
        .await
        .map_err(campus_services::CampusServiceError::user_message)?;
        persist_campus_service_session(&app, &state, persisted);
        *state.campus_wechat_payment_pending.lock().await = Some(pending);
    }
    let mut slot = state.campus_wechat_payment_pending.lock().await;
    let pending = slot
        .as_mut()
        .ok_or_else(|| "微信支付恢复失败".to_string())?;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        campus_services::check_wechat_card_recharge(pending),
    )
    .await
    .map_err(|_| "微信支付状态查询超时，请稍后重试".to_string())?
    .map_err(campus_services::CampusServiceError::user_message);
    match &result {
        Ok(status) if status.status == "paid" => {
            let _ = transition_recharge_transaction(
                &app,
                &state,
                &payment_id,
                recharge_state::RechargeStage::PaymentConfirmed,
                "微信支付状态已确认成功",
            );
            rust_log(&app, &state, "计费", "微信支付状态已确认成功", "success")
        }
        Ok(_) => rust_log(&app, &state, "计费", "微信支付尚未完成", "debug"),
        Err(error) => rust_log(&app, &state, "计费", error, "error"),
    }
    result
}

#[tauri::command]
async fn cancel_wechat_card_recharge(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    confirmation_id: Option<String>,
    payment_id: Option<String>,
) -> Result<(), String> {
    if let Some(confirmation_id) = confirmation_id {
        let mut slot = state.campus_wechat_recharge_pending.lock().await;
        if slot
            .as_ref()
            .is_some_and(|pending| pending.confirmation_id() == confirmation_id)
        {
            *slot = None;
            let _ = transition_recharge_transaction(
                &app,
                &state,
                &confirmation_id,
                recharge_state::RechargeStage::Cancelled,
                "用户在创建微信订单前取消",
            );
        }
    }
    if let Some(payment_id) = payment_id {
        let mut slot = state.campus_wechat_payment_pending.lock().await;
        if slot
            .as_ref()
            .is_some_and(|pending| pending.payment_id() == payment_id)
        {
            *slot = None;
        }
        let stage = state
            .config
            .read()
            .unwrap()
            .recharge_transactions
            .find(&payment_id)
            .map(|transaction| transaction.stage.clone());
        let transition = stage.and_then(recharge_state::stage_after_payment_context_closed);
        if let Some((next, note)) = transition {
            transition_recharge_transaction(&app, &state, &payment_id, next, note)?;
        }
    }
    Ok(())
}

async fn resolve_billing_access(
    app: &tauri::AppHandle,
    state: &AppState,
    account_user: Option<&str>,
    current_session: bool,
    allow_discovery: bool,
) -> Result<(billing::BillingAccess, VpnCompatibility), String> {
    if !current_session {
        let (account, compatibility) = billing_action_target(state, account_user)?;
        return Ok((
            billing::BillingAccess::saved(account.user, account.pass),
            compatibility,
        ));
    }
    ensure_billing_foreground(state)?;
    let expected_account = account_user
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if !allow_discovery && expected_account.is_none() {
        return Err("请先刷新当前登录账号的计费数据，再执行操作".to_string());
    }
    let compatibility = effective_vpn_compatibility(&state.config.read().unwrap());
    let generation = state.network_change_generation.load(Ordering::SeqCst);
    let network = get_network_info(app.clone(), Some(false));
    let route = portal_route_context_from_network(&network)?;
    let access = run_billing_read_while_foreground(state, async {
        tokio::time::timeout(
            std::time::Duration::from_secs(25),
            billing::current_session_access(
                compatibility,
                route.as_ref(),
                if allow_discovery {
                    None
                } else {
                    expected_account
                },
            ),
        )
        .await
        .map_err(|_| "当前登录账号的计费跳转超过 25 秒".to_string())?
        .map_err(|error| error.user_message())
    })
    .await?;
    if state.network_change_generation.load(Ordering::SeqCst) != generation {
        return Err("网络已变化，请重新打开当前登录账号的计费系统".to_string());
    }
    Ok((access, compatibility))
}

fn billing_action_target(
    state: &AppState,
    account_user: Option<&str>,
) -> Result<(Account, VpnCompatibility), String> {
    ensure_billing_foreground(state)?;
    // Billing may be reachable through a user-configured VPN even when Android's
    // physical/default transport is cellular. Never gate an explicit request on
    // transport type, and make sure a short-lived campus Wi-Fi binding is gone.
    #[cfg(target_os = "android")]
    clear_android_network_binding()?;
    let config = state.config.read().unwrap();
    let account = selected_billing_account(&config, account_user).ok_or_else(|| {
        if account_user.is_some() {
            "所选计费账号不存在或缺少有效配置".to_string()
        } else {
            "没有可用于计费系统的已保存账号".to_string()
        }
    })?;
    if account.pass.is_empty() {
        return Err("计费账号缺少已保存的密码".to_string());
    }
    Ok((account, effective_vpn_compatibility(&config)))
}

#[tauri::command]
async fn disconnect_billing_session(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    session_id: String,
    ip: String,
    mac: String,
    account_user: Option<String>,
    current_session: Option<bool>,
) -> Result<String, String> {
    let _fetch_guard = state.billing_fetch_lock.lock().await;
    let (access, compatibility) = resolve_billing_access(
        &app,
        &state,
        account_user.as_deref(),
        current_session.unwrap_or(false),
        false,
    )
    .await?;
    ensure_billing_foreground(&state)?;
    let result = run_billing_mutation_to_completion(&state, async {
        billing::disconnect_session(&access, compatibility, &session_id, &ip, &mac)
            .await
            .map_err(|error| error.user_message())
    })
    .await;
    state
        .billing_sessions
        .lock()
        .unwrap()
        .remove(&access.account);
    match &result {
        Ok(_) => rust_log(
            &app,
            &state,
            "计费",
            "用户已确认注销一条在线会话",
            "success",
        ),
        Err(error) => rust_log(&app, &state, "计费", error, "error"),
    }
    result
}

#[tauri::command]
async fn set_billing_mauth(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    enabled: bool,
    account_user: Option<String>,
    current_session: Option<bool>,
) -> Result<String, String> {
    let _fetch_guard = state.billing_fetch_lock.lock().await;
    let (access, compatibility) = resolve_billing_access(
        &app,
        &state,
        account_user.as_deref(),
        current_session.unwrap_or(false),
        false,
    )
    .await?;
    ensure_billing_foreground(&state)?;
    let result = run_billing_mutation_to_completion(&state, async {
        billing::set_mauth_enabled(&access, compatibility, enabled)
            .await
            .map_err(|error| error.user_message())
    })
    .await;
    state
        .billing_sessions
        .lock()
        .unwrap()
        .remove(&access.account);
    match &result {
        Ok(_) => rust_log(
            &app,
            &state,
            "计费",
            "用户已确认修改无感认证状态",
            "success",
        ),
        Err(error) => rust_log(&app, &state, "计费", error, "error"),
    }
    result
}

fn schedule_network_change_readiness(app: tauri::AppHandle, state: Arc<AppState>) {
    connectivity::invalidate(&app, &state);
    network_events::record(
        &app,
        &state,
        "change",
        "网络变化或网卡选择已更新，正在等待地址稳定并重新检测",
    );
    state.network_schedule.lock().unwrap().reset();
    *state.link_health.lock().unwrap() = None;
    let generation = state
        .network_change_generation
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1);
    let _ = app.emit(
        "link-health-reset",
        serde_json::json!({"generation": generation}),
    );
    let progress = network_progress::Run::begin(
        &app,
        &state,
        generation,
        "检测到 IP 或网络变化，正在等待新的 IP 分配",
    );
    state.network_change_waiting.store(true, Ordering::SeqCst);
    let mut checking_payload = state.last_network_state.lock().unwrap().clone();
    if let Some(object) = checking_payload.as_object_mut() {
        object.insert("state".to_string(), serde_json::json!("Checking"));
        object.remove("loginMessage");
        object.insert(
            "timestamp".to_string(),
            serde_json::json!(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
        );
    }
    *state.last_network_state.lock().unwrap() = checking_payload.clone();
    let _ = app.emit("network-state-change", checking_payload);

    tauri::async_runtime::spawn(async move {
        let mut ready_ip = String::new();
        for attempt in 1..=10 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if state.network_change_generation.load(Ordering::SeqCst) != generation {
                return;
            }
            let network = get_network_info(app.clone(), Some(false));
            let current_ip = network
                .get("ip")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim();
            if usable_physical_ipv4(current_ip).is_some() {
                ready_ip = current_ip.to_string();
                progress.phase("已取得物理网卡 IP，准备完整检测", 20);
                rust_log(
                    &app,
                    &state,
                    "网络",
                    &format!(
                        "网络变化后第 {attempt} 次检查取得新的物理接口 IPv4：{ready_ip}，开始完整检测"
                    ),
                    "info",
                );
                break;
            }
            progress.phase(
                &format!("正在等待新的 IP 分配（第 {attempt}/10 次检查）"),
                5 + attempt,
            );
            rust_log(
                &app,
                &state,
                "网络",
                &format!("网络变化后第 {attempt}/10 次检查仍未取得物理接口 IPv4"),
                "debug",
            );
        }
        if state.network_change_generation.load(Ordering::SeqCst) != generation {
            return;
        }
        state.network_change_waiting.store(false, Ordering::SeqCst);
        if ready_ip.is_empty() {
            rust_log(
                &app,
                &state,
                "网络",
                "网络变化后连续 10 次仍未取得物理接口 IPv4，将执行一次完整检测以更新离线状态",
                "info",
            );
        }
        *state.last_known_ip.lock().unwrap() = Some(ready_ip);
        progress.handoff();
        trigger_network_check(app, state, true).await;
    });
}

#[tauri::command]
fn notify_network_change(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    source: Option<String>,
) {
    state.is_suspended.store(false, Ordering::SeqCst);
    state.non_campus_count.store(0, Ordering::SeqCst);
    rust_log(
        &app,
        &state,
        "网络",
        &format!(
            "收到{}网络变化事件，等待新的物理接口 IPv4 就绪后执行完整检测",
            source.unwrap_or_else(|| "系统".to_string()),
        ),
        "info",
    );
    schedule_network_change_readiness(app, state.inner().clone());
}

#[tauri::command]
fn set_auto_login_pause(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    minutes: i64,
) -> i64 {
    let until = if minutes <= 0 {
        0
    } else {
        chrono::Utc::now().timestamp() + minutes.saturating_mul(60)
    };
    state.auto_login_paused_until.store(until, Ordering::SeqCst);
    rust_log(
        &app,
        &state,
        "自动登录",
        if until == 0 {
            "已恢复自动登录"
        } else {
            "已暂停自动登录 1 小时"
        },
        "info",
    );
    #[cfg(desktop)]
    refresh_tray_menu(&app, &state);
    until
}

#[tauri::command]
fn log_from_js(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    module: String,
    message: String,
    log_type: String,
) {
    rust_log(&app, &state, &module, &message, &log_type);
}

fn update_billing_background_lifecycle(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    is_background: bool,
) {
    let previous = state.is_in_background.swap(is_background, Ordering::SeqCst);
    if previous == is_background {
        return;
    }
    let generation = state
        .billing_session_expiry_generation
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1);
    if !is_background {
        return;
    }
    let app = app.clone();
    let state = state.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(15 * 60)).await;
        if state.is_in_background.load(Ordering::SeqCst)
            && state
                .billing_session_expiry_generation
                .load(Ordering::SeqCst)
                == generation
        {
            let removed = state.billing_sessions.lock().unwrap().clear();
            if removed > 0 {
                rust_log(
                    &app,
                    &state,
                    "计费",
                    &format!("App 已在后台停留 15 分钟，已销毁 {removed} 个计费登录会话"),
                    "debug",
                );
            }
        }
    });
}

#[tauri::command]
fn set_background_state(app: tauri::AppHandle, state: tauri::State<Arc<AppState>>, is_bg: bool) {
    update_billing_background_lifecycle(&app, state.inner(), is_bg);
    if !is_bg {
        // A headless/background login may complete while the WebView is not
        // allowed to perform billing reads. Ask the foreground UI to execute
        // one coalesced dashboard refresh as soon as it is visible again.
        let _ = app.emit(
            "dashboard-user-info-refresh",
            serde_json::json!({"reason": "foreground-resume"}),
        );
    }
    #[cfg(not(target_os = "android"))]
    let _ = &app;
    #[cfg(target_os = "android")]
    if !is_bg {
        // The headless JNI engine writes the same cooldown file while the
        // WebView is backgrounded. Refresh the live state before another
        // foreground/manual attempt can bypass those updates.
        *state.account_health.lock().unwrap() = load_account_health(&app);
        emit_account_health(&app, &state);
    }
    let val = state.countdown.load(Ordering::SeqCst);
    let cfg = state.config.read().unwrap();
    let configured_interval = if is_bg {
        cfg.check_interval_bg
    } else {
        cfg.check_interval
    };
    let mobile_data = is_mobile_data_network(&state.last_network_state.lock().unwrap());
    let max_interval = if mobile_data {
        mobile_data_check_interval(configured_interval, is_bg)
    } else {
        configured_interval
    };
    if val > max_interval {
        state.countdown.store(max_interval, Ordering::SeqCst);
    }
}

#[tauri::command]
fn get_current_network_state(state: tauri::State<Arc<AppState>>) -> serde_json::Value {
    state.last_network_state.lock().unwrap().clone()
}

#[cfg(desktop)]
fn refresh_tray_menu(app: &tauri::AppHandle, state: &AppState) {
    use tauri::menu::{MenuBuilder, MenuItem};
    let network_state = state.last_network_state.lock().unwrap().clone();
    let state_name = match network_state.get("state").and_then(|value| value.as_str()) {
        Some("Online") => "已联网",
        Some("BjutCampus") => "校园网待认证",
        _ => "离线",
    };
    let paused =
        state.auto_login_paused_until.load(Ordering::SeqCst) > chrono::Utc::now().timestamp();
    let config = state.config.read().unwrap().clone();
    let status_item = match MenuItem::with_id(
        app,
        "status",
        format!("状态：{state_name}"),
        false,
        None::<&str>,
    ) {
        Ok(item) => item,
        Err(_) => return,
    };
    let mut builder = MenuBuilder::new(app)
        .item(&status_item)
        .separator()
        .text("show", "显示主窗口")
        .text("check", "立即检测网络")
        .text("login", "立即登录")
        .text(
            "pause",
            if paused {
                "恢复自动登录"
            } else {
                "暂停自动登录 1 小时"
            },
        )
        .separator();
    for (index, account) in config
        .accounts
        .iter()
        .enumerate()
        .filter(|(_, account)| !account.is_disabled.unwrap_or(false))
    {
        let marker = if account.is_default { "✓" } else { " " };
        builder = builder.text(
            format!("account:{index}"),
            format!("{marker} 首选账号：{}", account.user),
        );
    }
    let menu = match builder.separator().text("quit", "退出").build() {
        Ok(menu) => menu,
        Err(_) => return,
    };
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_menu(Some(menu));
        let _ = tray.set_tooltip(Some(format!("BJUT-AL · {state_name}")));
    }
}

#[cfg(desktop)]
async fn tray_manual_login(app: tauri::AppHandle, state: Arc<AppState>) {
    let network = get_network_info(app.clone(), Some(true));
    let transport = network_transport(&network).to_string();
    let ssid = network
        .get("ssid")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let bssid = network
        .get("bssid")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let config = state.config.read().unwrap().clone();
    let compatibility = effective_vpn_compatibility(&config);
    let portal_route_context = match portal_route_context_from_network(&network) {
        Ok(context) => context,
        Err(reason) => {
            rust_log(
                &app,
                &state,
                "安全",
                &format!("托盘登录已阻止：{reason}"),
                "error",
            );
            let _ = show_native_notification(&app, "校园网登录已阻止", &reason);
            return;
        }
    };
    let detection = detect_login_type_details_rust(
        compatibility,
        &ssid,
        &transport,
        portal_route_context.as_ref(),
    )
    .await;
    if type1_portal_requires_maximum(&detection, compatibility) {
        rust_log(
            &app,
            &state,
            "托盘",
            TYPE1_MAXIMUM_MODE_REQUIRED_MESSAGE,
            "info",
        );
        let _ = show_native_notification(
            &app,
            "校园网登录需要确认",
            "已发现宿舍网认证入口，请在设置中临时启用最高兼容模式后重试",
        );
        return;
    }
    let detected = if detection.login_ready {
        detection.login_type.clone()
    } else {
        LoginType::Unknown
    };
    let fresh_network = get_network_info(app.clone(), Some(true));
    let fresh_route_context = match portal_route_context_from_network(&fresh_network) {
        Ok(context) => context,
        Err(reason) => {
            rust_log(
                &app,
                &state,
                "安全",
                &format!("托盘登录已阻止：{reason}"),
                "error",
            );
            let _ = show_native_notification(&app, "校园网登录已阻止", &reason);
            return;
        }
    };
    let fresh_transport = network_transport(&fresh_network).to_string();
    let fresh_ssid = fresh_network
        .get("ssid")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let fresh_bssid = fresh_network
        .get("bssid")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let same_wifi_identity = !transport.eq_ignore_ascii_case("wifi")
        || same_exact_wifi_identity(&ssid, &bssid, &fresh_ssid, &fresh_bssid);
    if portal_route_context != fresh_route_context
        || !transport.eq_ignore_ascii_case(&fresh_transport)
        || !same_wifi_identity
    {
        let reason = "认证网关探测期间物理接口、IP 或 Wi-Fi 身份发生变化，请重新登录";
        rust_log(
            &app,
            &state,
            "安全",
            &format!("托盘登录已阻止：{reason}"),
            "error",
        );
        let _ = show_native_notification(&app, "校园网登录已阻止", reason);
        return;
    }
    let network = fresh_network;
    let portal_route_context = fresh_route_context;
    let transport = fresh_transport;
    let ssid = fresh_ssid;
    let bssid = fresh_bssid;
    let ip = network
        .get("ip")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let profile = matching_network_profile(&config, &ssid, &bssid, &detected);
    let login_type = profile
        .as_ref()
        .and_then(|item| login_type_from_profile(&item.login_type))
        .unwrap_or(detected);
    if login_type == LoginType::Unknown {
        let _ = show_native_notification(&app, "校园网登录", "未检测到可用的校园网认证网关");
        return;
    }
    if let Err(reason) = automatic_login_network_allowed(NetworkTrustInput {
        login_type: &login_type,
        ssid: &ssid,
        bssid: &bssid,
        ip: &ip,
        transport: &transport,
        identity_fresh: network_identity_is_fresh(&network),
        whitelist: &config.whitelist,
        blacklist: &config.blacklist,
    }) {
        rust_log(
            &app,
            &state,
            "安全",
            &format!("托盘登录已阻止：{reason}"),
            "error",
        );
        let _ = show_native_notification(&app, "校园网登录已阻止", &reason);
        return;
    }
    let accounts = accounts_for_profile(config.accounts, profile.as_ref());
    let account = accounts
        .iter()
        .find(|account| account.is_default)
        .or_else(|| accounts.first());
    let Some(account) = account else {
        let _ = show_native_notification(&app, "校园网登录", "没有可用且已保存密码的账号");
        return;
    };
    if let Err(remaining) = account_attempt_allowed(&state, &account.user) {
        let _ = show_native_notification(
            &app,
            "账号正在冷却",
            &format!("请在 {remaining} 秒后重试或在诊断页解除"),
        );
        return;
    }
    rust_log(
        &app,
        &state,
        "托盘",
        &format!("使用首选账号 {} 执行快捷登录", account.user),
        "info",
    );
    match login_to_campus_network_rust(
        login_type,
        &account.user,
        &account.pass,
        compatibility,
        portal_route_context.as_ref(),
    )
    .await
    {
        Ok((true, message)) => {
            record_account_success(&app, &state, &account.user);
            rust_log(
                &app,
                &state,
                "托盘",
                &format!("快捷登录成功：{message}"),
                "success",
            );
            let _ = show_native_notification(
                &app,
                "校园网登录成功",
                &format!("账号：{}", account.user),
            );
            trigger_network_check(app, state, true).await;
        }
        Ok((false, message)) => {
            record_account_failure(&app, &state, &account.user, &message);
            rust_log(
                &app,
                &state,
                "托盘",
                &format!("快捷登录失败：{message}"),
                "error",
            );
            let _ = show_native_notification(&app, "校园网登录失败", &message);
        }
        Err(error) => {
            if login_result_is_ambiguous(&error) {
                rust_log(
                    &app,
                    &state,
                    "托盘",
                    &format!("快捷登录结果无法确认：{error}"),
                    "error",
                );
                let _ = show_native_notification(
                    &app,
                    "校园网登录结果待确认",
                    "请求已经发送，请先检查网络状态，不要立即重复登录",
                );
                return;
            }
            record_account_failure(&app, &state, &account.user, &format!("请求出错: {error}"));
            rust_log(
                &app,
                &state,
                "托盘",
                &format!("快捷登录出错：{error}"),
                "error",
            );
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().setup(|_app| {
        #[cfg(target_os = "android")]
        _app.handle().plugin(tauri_plugin_barcode_scanner::init())?;
        #[cfg(desktop)]
        {
            // Set frameless for non-macOS desktop windows
            #[cfg(not(target_os = "macos"))]
            {
                if let Some(window) = _app.get_webview_window("main") {
                    let _ = window.set_decorations(false);
                    let _ = window.set_shadow(true);
                    #[cfg(target_os = "windows")]
                    {
                        let (width, height) = window
                            .current_monitor()
                            .ok()
                            .flatten()
                            .map(|monitor| {
                                let screen =
                                    monitor.size().to_logical::<f64>(monitor.scale_factor());
                                (
                                    1080.0_f64.min((screen.width - 48.0).max(320.0)),
                                    780.0_f64.min((screen.height - 80.0).max(400.0)),
                                )
                            })
                            .unwrap_or((1080.0, 780.0));
                        let _ = window.set_size(tauri::LogicalSize::new(width, height));
                        let _ = window.center();
                    }
                }
            }

            if let Some(window) = _app.get_webview_window("main") {
                window_geometry::install(&window);
            }
        }
        let app_state = std::sync::Arc::new(AppState {
            config: RwLock::new(AppConfig {
                preferred_interface: String::new(),
                adaptive_network_checks: true,
                accounts: Vec::new(),
                auto_login: false,
                check_interval: 15,
                check_interval_bg: 60,
                wifi_change_detect: true,
                log_level: "info".to_string(),
                theme: default_theme(),
                accent_color: default_accent_color(),
                color_mode: default_color_mode(),
                macos_dock_visible: None,
                vpn_compatibility: default_vpn_compatibility(),
                vpn_maximum_until: None,
                whitelist: Vec::new(),
                blacklist: Vec::new(),
                network_profiles: Vec::new(),
                usage_alerts: true,
                balance_alert_threshold: default_balance_alert_threshold(),
                flow_alert_threshold: default_flow_alert_threshold(),
                android_notification_mode: default_android_notification_mode(),
                android_notify_network_status: true,
                android_notify_login_results: true,
                android_notify_background_errors: true,
                campus_service_sessions: Vec::new(),
                recharge_transactions: recharge_state::RechargeJournal::default(),
            }),
            credential_storage_status: Mutex::new("unknown".to_string()),
            account_health: Mutex::new(load_account_health(_app.handle())),
            logs: Mutex::new(Vec::new()),
            countdown: AtomicI32::new(15),
            is_checking: AtomicBool::new(false),
            pending_full_check: AtomicBool::new(false),
            network_change_waiting: AtomicBool::new(false),
            network_change_generation: AtomicU64::new(0),
            login_operation_generation: AtomicU64::new(0),
            manual_login_in_progress: AtomicBool::new(false),
            login_progress: login_progress::LoginProgressControl::default(),
            login_request_lock: tokio::sync::Mutex::new(()),
            is_suspended: AtomicBool::new(false),
            last_known_ip: Mutex::new(None),
            non_campus_count: AtomicU32::new(0),
            is_in_background: AtomicBool::new(false),
            link_health: Mutex::new(None),
            connectivity: connectivity::ProbePool::default(),
            network_events: Mutex::new(network_events::load(_app.handle())),
            system_online: AtomicBool::new(false),
            network_schedule: Mutex::new(network_schedule::AdaptiveSchedule::default()),
            network_progress: network_progress::Control::default(),
            last_network_state: Mutex::new(serde_json::json!({
                "state": "Checking",
                "ssid": "",
                "bssid": "",
                "ip": "",
                "timestamp": "--"
            })),
            auto_login_paused_until: std::sync::atomic::AtomicI64::new(0),
            usage_alert_history: Mutex::new(HashMap::new()),
            update_download: UpdateDownloadControl::default(),
            billing_fetch_lock: tokio::sync::Mutex::new(()),
            billing_sessions: Mutex::new(billing::BillingSessionPool::default()),
            billing_session_expiry_generation: AtomicU64::new(0),
            campus_service_lock: tokio::sync::Mutex::new(()),
            campus_recharge_pending: tokio::sync::Mutex::new(None),
            campus_alipay_recharge_pending: tokio::sync::Mutex::new(None),
            campus_wechat_recharge_pending: tokio::sync::Mutex::new(None),
            campus_wechat_payment_pending: tokio::sync::Mutex::new(None),
            pending_discovered_account: tokio::sync::Mutex::new(None),
        });
        _app.manage(app_state.clone());

        let app_handle = _app.handle().clone();
        let state_clone = app_state.clone();

        #[cfg(target_os = "macos")]
        install_macos_panic_log_hook(&app_handle);

        // Load config on startup
        load_config(&app_handle, &state_clone);

        // AppKit activation policy belongs to application setup. Applying the
        // saved policy before the WebView asks to reveal the window avoids a
        // cold-start race between accessory-mode changes and Tao window events.
        #[cfg(target_os = "macos")]
        if let Some(visible) = state_clone
            .config
            .read()
            .ok()
            .and_then(|config| config.macos_dock_visible)
        {
            if let Err(error) = apply_macos_activation_policy(&app_handle, visible) {
                eprintln!("Unable to restore macOS activation policy: {error}");
            }
        }

        initialize_log_history(&app_handle, &state_clone);

        // Start background loop task in Rust
        let loop_handle = app_handle.clone();
        let loop_state = state_clone.clone();
        tauri::async_runtime::spawn(async move {
            let mut wifi_check_counter = 0;
            let mut previous_tick = std::time::SystemTime::now();
            loop {
                let relaxed = loop_state.is_in_background.load(Ordering::SeqCst)
                    && loop_state
                        .network_schedule
                        .lock()
                        .unwrap()
                        .plan
                        .interface_poll_seconds
                        >= 15;
                let tick_seconds = if relaxed { 5 } else { 1 };
                tokio::time::sleep(std::time::Duration::from_secs(tick_seconds)).await;
                let elapsed = previous_tick.elapsed().unwrap_or_default();
                previous_tick = std::time::SystemTime::now();
                if elapsed.as_secs() > 20 {
                    loop_state.is_suspended.store(false, Ordering::SeqCst);
                    schedule_network_change_readiness(loop_handle.clone(), loop_state.clone());
                    continue;
                }
                if loop_state.manual_login_in_progress.load(Ordering::SeqCst) {
                    continue;
                }
                let is_bg = loop_state.is_in_background.load(Ordering::SeqCst);
                let is_susp = loop_state.is_suspended.load(Ordering::SeqCst);
                let is_chk = loop_state.is_checking.load(Ordering::SeqCst);

                if loop_state.network_change_waiting.load(Ordering::SeqCst) {
                    let _ = loop_handle
                        .emit("countdown-tick", serde_json::json!({"status": "checking"}));
                    continue;
                }

                if !is_chk && loop_state.pending_full_check.swap(false, Ordering::SeqCst) {
                    rust_log(
                        &loop_handle,
                        &loop_state,
                        "网络",
                        "执行等待中的完整网络检测",
                        "debug",
                    );
                    trigger_network_check(loop_handle.clone(), loop_state.clone(), true).await;
                    continue;
                }

                // 1. Local interface change check. Android NetworkCallback is the
                // primary signal on mobile data, so polling can be much slower there.
                wifi_check_counter += tick_seconds as i32;
                let mobile_data_active =
                    is_mobile_data_network(&loop_state.last_network_state.lock().unwrap());
                let interface_poll_interval = if mobile_data_active {
                    30
                } else {
                    loop_state
                        .network_schedule
                        .lock()
                        .unwrap()
                        .plan
                        .interface_poll_seconds
                };
                if wifi_check_counter >= interface_poll_interval {
                    wifi_check_counter = 0;
                    let wifi_change_detect = {
                        let cfg = loop_state.config.read().unwrap();
                        cfg.wifi_change_detect
                    };
                    if wifi_change_detect {
                        let (ip_changed, current_ip, last_ip) = {
                            let current_ip = get_local_ip(loop_handle.clone());
                            let mut last_ip_lock = loop_state.last_known_ip.lock().unwrap();
                            let last_ip = last_ip_lock.clone();
                            let changed =
                                network_ip_observation_changed(last_ip.as_deref(), &current_ip);
                            // Preserve the empty state as an observation. This
                            // makes an interface transition old -> empty -> new
                            // visible instead of treating the new address as a
                            // fresh startup value and skipping the full check.
                            *last_ip_lock = Some(current_ip.clone());
                            (changed, current_ip, last_ip)
                        };
                        rust_log(
                            &loop_handle,
                            &loop_state,
                            "网络",
                            &format!(
                                "[DEBUG] 执行网络接口变更检测。当前 IP: {} (上次 IP: {})",
                                current_ip,
                                last_ip.as_deref().unwrap_or("空")
                            ),
                            "debug",
                        );
                        if ip_changed {
                            rust_log(
                                &loop_handle,
                                &loop_state,
                                "网络",
                                &format!(
                                    "检测到局域网 IP 发生变更: {} -> {}，等待地址稳定...",
                                    last_ip.unwrap_or_default(),
                                    if current_ip.is_empty() {
                                        "未分配"
                                    } else {
                                        &current_ip
                                    }
                                ),
                                "info",
                            );
                            loop_state.is_suspended.store(false, Ordering::SeqCst);
                            loop_state.non_campus_count.store(0, Ordering::SeqCst);
                            schedule_network_change_readiness(
                                loop_handle.clone(),
                                loop_state.clone(),
                            );
                            continue;
                        }
                    }
                }

                // 2. Connectivity Check Loop (every 1 second)
                if !is_chk {
                    if !is_bg && is_susp {
                        rust_log(
                            &loop_handle,
                            &loop_state,
                            "网络",
                            "检测到已返回前台，恢复连通性检测...",
                            "info",
                        );
                        loop_state.is_suspended.store(false, Ordering::SeqCst);
                        loop_state.non_campus_count.store(0, Ordering::SeqCst);
                        trigger_network_check(loop_handle.clone(), loop_state.clone(), true).await;
                        continue;
                    }
                    if is_susp {
                        let _ = loop_handle
                            .emit("countdown-tick", serde_json::json!({"status": "suspended"}));
                        continue;
                    }
                    let val = loop_state
                        .countdown
                        .fetch_sub(tick_seconds as i32, Ordering::SeqCst);
                    let current_countdown = val - tick_seconds as i32;
                    if current_countdown <= 0 {
                        rust_log(
                            &loop_handle,
                            &loop_state,
                            "网络",
                            "[DEBUG] 倒计时归零，触发自动网络连通性检测",
                            "debug",
                        );
                        trigger_network_check(loop_handle.clone(), loop_state.clone(), !is_bg)
                            .await;
                    } else {
                        let _ = loop_handle.emit(
                            "countdown-tick",
                            serde_json::json!({
                                "status": "ticking",
                                "seconds": current_countdown
                            }),
                        );
                    }
                } else {
                    let _ = loop_handle
                        .emit("countdown-tick", serde_json::json!({"status": "checking"}));
                }
            }
        });

        let init_handle = app_handle.clone();
        let init_state = state_clone.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let full_details = !app_is_in_background(&init_handle, &init_state);
            trigger_network_check(init_handle, init_state, full_details).await;
        });

        #[cfg(desktop)]
        {
            use tauri::Manager;

            // Prevent window close, hide instead to keep in system tray
            if let Some(window) = _app.get_webview_window("main") {
                let window_clone = window.clone();
                let window_state = state_clone.clone();
                window.on_window_event(move |event| match event {
                    tauri::WindowEvent::Focused(focused) => {
                        let visible = window_clone.is_visible().unwrap_or(false);
                        let minimized = window_clone.is_minimized().unwrap_or(false);
                        update_billing_background_lifecycle(
                            window_clone.app_handle(),
                            &window_state,
                            desktop_window_background_state(
                                visible,
                                *focused,
                                minimized,
                                !cfg!(target_os = "windows"),
                            ),
                        );
                    }
                    tauri::WindowEvent::Resized(_) if cfg!(target_os = "windows") => {
                        let visible = window_clone.is_visible().unwrap_or(false);
                        let focused = window_clone.is_focused().unwrap_or(false);
                        let minimized = window_clone.is_minimized().unwrap_or(false);
                        update_billing_background_lifecycle(
                            window_clone.app_handle(),
                            &window_state,
                            desktop_window_background_state(visible, focused, minimized, false),
                        );
                    }
                    tauri::WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        update_billing_background_lifecycle(
                            window_clone.app_handle(),
                            &window_state,
                            true,
                        );
                        let _ = window_clone.hide();
                    }
                    _ => {}
                });
            }

            // System Tray Setup
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

            let mut tray_builder = TrayIconBuilder::with_id("main-tray");

            #[cfg(target_os = "macos")]
            {
                let mac_icon =
                    tauri::image::Image::from_bytes(include_bytes!("../icons/tray_mac.png"))
                        .expect("Failed to load macOS tray icon");
                tray_builder = tray_builder.icon(mac_icon);
            }
            #[cfg(not(target_os = "macos"))]
            {
                if let Some(ic) = _app.default_window_icon().cloned() {
                    tray_builder = tray_builder.icon(ic);
                }
            }

            let _tray = tray_builder
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id == "show" {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    } else if event.id == "check" {
                        let state = app.state::<Arc<AppState>>().inner().clone();
                        let app_handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            trigger_network_check(app_handle, state, true).await;
                        });
                    } else if event.id == "login" {
                        let state = app.state::<Arc<AppState>>().inner().clone();
                        let app_handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            tray_manual_login(app_handle, state).await;
                        });
                    } else if event.id == "pause" {
                        let state = app.state::<Arc<AppState>>();
                        let paused = state.auto_login_paused_until.load(Ordering::SeqCst)
                            > chrono::Utc::now().timestamp();
                        state.auto_login_paused_until.store(
                            if paused {
                                0
                            } else {
                                chrono::Utc::now().timestamp() + 3600
                            },
                            Ordering::SeqCst,
                        );
                        rust_log(
                            app,
                            &state,
                            "托盘",
                            if paused {
                                "已恢复自动登录"
                            } else {
                                "已暂停自动登录 1 小时"
                            },
                            "info",
                        );
                        refresh_tray_menu(app, &state);
                    } else if let Some(index) = event
                        .id
                        .as_ref()
                        .strip_prefix("account:")
                        .and_then(|value| value.parse::<usize>().ok())
                    {
                        let state = app.state::<Arc<AppState>>();
                        let mut config = state.config.read().unwrap().clone();
                        if let Some(preferred_user) =
                            promote_default_account(&mut config.accounts, index)
                        {
                            if save_config(app, &state, config).is_ok() {
                                rust_log(app, &state, "托盘", "已切换首选账号", "info");
                                let _ = app.emit(
                                    "preferred-account-change",
                                    serde_json::json!({ "index": 0, "user": preferred_user }),
                                );
                                refresh_tray_menu(app, &state);
                            }
                        }
                    } else if event.id == "quit" {
                        app.exit(0);
                    }
                })
                .on_tray_icon_event(|tray_event, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray_event.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(_app)?;

            refresh_tray_menu(_app.handle(), &state_clone);

            #[cfg(target_os = "macos")]
            {
                let _ = _tray.set_icon_as_template(true);
            }
        }
        Ok(())
    });

    #[allow(unused_mut)]
    let mut builder = builder
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init());

    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_autostart::Builder::default().build())
            .plugin(tauri_plugin_updater::Builder::new().build());
    }

    let app = builder
        .invoke_handler(tauri::generate_handler![
            get_network_info,
            get_network_adapters,
            get_link_health,
            network_events::get_network_events,
            get_network_schedule,
            get_network_check_progress,
            set_preferred_interface,
            request_battery_optimizations,
            request_foreground_permissions,
            request_background_permissions,
            start_keep_alive_service,
            stop_keep_alive_service,
            get_local_ip,
            exit_app,
            get_macos_dock_visible,
            set_dock_visible,
            frontend_ready,
            read_clipboard,
            write_clipboard,
            export_config_backup,
            import_config_backup,
            sync_config,
            get_app_config,
            verify_legacy_credential_fingerprint,
            get_account_password,
            get_credential_storage_status,
            get_credential_storage_health,
            get_account_health,
            reset_account_health,
            network_diagnostics::run_network_diagnostics,
            trusted_time::get_network_time,
            restart_lgn_adapter,
            network_diagnostics::create_diagnostic_bundle,
            get_logs,
            get_log_text,
            export_logs,
            export_billing_csv,
            clear_all_logs,
            get_countdown_status,
            trigger_manual_check,
            evaluate_manual_network_trust,
            set_current_network_trust,
            set_named_network_trust,
            remove_saved_network_trust,
            manual_login,
            cancel_manual_login,
            logout_current_campus_session,
            get_user_info,
            get_remaining_flow,
            discover_current_campus_account,
            accept_discovered_campus_account,
            reject_discovered_campus_account,
            get_billing_center,
            query_billing_records,
            perform_billing_action,
            prepare_network_recharge,
            confirm_network_recharge,
            get_recoverable_recharges,
            finish_recharge_recovery,
            get_network_recharge_balances,
            cancel_network_recharge,
            prepare_alipay_card_recharge,
            confirm_alipay_card_recharge,
            cancel_alipay_card_recharge,
            prepare_wechat_card_recharge,
            confirm_wechat_card_recharge,
            check_wechat_card_recharge,
            cancel_wechat_card_recharge,
            disconnect_billing_session,
            set_billing_mauth,
            notify_network_change,
            set_auto_login_pause,
            get_update_target,
            fetch_latest_official_release_tag,
            fetch_official_update_manifest,
            get_release_asset_size,
            control_update_download,
            download_and_install_update,
            reinstall_current_version,
            log_from_js,
            set_background_state,
            get_current_network_state
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        #[cfg(target_os = "macos")]
        {
            use tauri::Manager;
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        }
        let _ = app_handle;
        let _ = event;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_download_control_releases_exclusive_state_on_drop() {
        let control = UpdateDownloadControl::default();
        let guard = control.begin().unwrap();
        assert!(control.begin().is_err());
        control.paused.store(true, Ordering::SeqCst);
        drop(guard);
        assert!(!control.active.load(Ordering::SeqCst));
        assert!(!control.paused.load(Ordering::SeqCst));
        assert!(control.begin().is_ok());
    }

    #[test]
    fn mobile_data_runtime_policy_is_android_only() {
        assert_eq!(
            is_mobile_data_network(&serde_json::json!({"transport": "cellular"})),
            cfg!(target_os = "android")
        );
        assert_eq!(
            is_mobile_data_network(&serde_json::json!({"transport": "CELLULAR"})),
            cfg!(target_os = "android")
        );
        assert!(!is_mobile_data_network(&serde_json::json!({
            "ssid": "<unknown ssid>",
            "bssid": "00:00:00:00:00:00",
            "ip": "0.0.0.0"
        })));
        assert!(!is_mobile_data_network(
            &serde_json::json!({"transport": "wifi"})
        ));
    }

    #[test]
    fn ip_change_detection_preserves_empty_transition_state() {
        assert!(!network_ip_observation_changed(None, "10.126.0.2"));
        assert!(network_ip_observation_changed(Some("10.126.0.2"), ""));
        assert!(network_ip_observation_changed(Some(""), "10.126.0.3"));
        assert!(!network_ip_observation_changed(
            Some("10.126.0.3"),
            "10.126.0.3"
        ));
    }

    #[test]
    fn desktop_background_policy_can_ignore_unreliable_focus() {
        assert!(!desktop_window_background_state(true, false, false, false));
        assert!(desktop_window_background_state(true, false, false, true));
        assert!(desktop_window_background_state(false, true, false, false));
        assert!(desktop_window_background_state(true, true, true, false));
        assert!(!desktop_window_background_state(true, true, false, true));
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn desktop_auto_login_requires_a_same_interface_identity() {
        assert!(network_identity_is_fresh(
            &serde_json::json!({"identitySource": "sameInterface"})
        ));
        for source in ["routeFallback", "unverifiedFallback", "unknown", ""] {
            assert!(!network_identity_is_fresh(
                &serde_json::json!({"identitySource": source})
            ));
        }
    }

    #[test]
    fn wired_type3_classification_accepts_observed_campus_client_ranges() {
        for allowed in [
            "10.21.1.2",
            "10.26.1.2",
            "10.126.1.2",
            "172.17.0.2",
            "172.26.33.104",
            "172.30.201.2",
        ] {
            assert!(is_campus_wired_ipv4(allowed), "{allowed}");
        }
        for rejected in [
            "10.0.1.2",
            "10.28.1.2",
            "172.16.0.2",
            "172.29.255.1",
            "172.31.0.1",
            "198.18.1.2",
            "0.0.0.0",
        ] {
            assert!(!is_campus_wired_ipv4(rejected), "{rejected}");
        }
        assert!(is_lgn_wired_client_ipv4("172.26.33.104"));
        assert!(!is_lgn_wired_client_ipv4("10.126.80.236"));
        assert!(!is_lgn_wired_client_ipv4("172.30.201.2"));
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn desktop_portal_route_context_requires_same_interface_metadata() {
        let valid = serde_json::json!({
            "interfaceName": "en0",
            "identitySource": "sameInterface",
            "ip": "10.26.1.2"
        });
        let route = portal_route_context_from_network(&valid)
            .unwrap()
            .expect("desktop route context");
        assert_eq!(route.interface_name(), "en0");
        assert_eq!(route.physical_ipv4().to_string(), "10.26.1.2");

        for invalid in [
            serde_json::json!({
                "interfaceName": "en0",
                "identitySource": "unverifiedFallback",
                "ip": "10.26.1.2"
            }),
            serde_json::json!({
                "interfaceName": "",
                "identitySource": "sameInterface",
                "ip": "10.26.1.2"
            }),
            serde_json::json!({
                "interfaceName": "en0",
                "identitySource": "sameInterface",
                "ip": "198.18.1.2"
            }),
        ] {
            assert!(portal_route_context_from_network(&invalid).is_err());
        }
    }

    #[test]
    fn manual_login_snapshot_detects_every_identity_change() {
        let network = serde_json::json!({
            "networkId": "42",
            "interfaceName": "wlan0",
            "transport": "wifi",
            "ssid": "bjut_wifi",
            "bssid": "AA-BB-CC-DD-EE-FF",
            "ip": "10.26.1.2"
        });
        let expected = NetworkIdentitySnapshot::capture(&network);
        assert!(ensure_same_network_identity(&expected, &network).is_ok());

        for (field, value) in [
            ("networkId", "43"),
            ("interfaceName", "wlan1"),
            ("transport", "ethernet"),
            ("ssid", "bjut-wifi"),
            ("bssid", "aa:bb:cc:dd:ee:00"),
            ("ip", "10.26.1.3"),
        ] {
            let mut changed = network.clone();
            changed[field] = serde_json::json!(value);
            assert!(
                ensure_same_network_identity(&expected, &changed).is_err(),
                "{field} change must stop credential submission"
            );
        }

        assert!(same_exact_wifi_identity(
            "bjut_wifi",
            "AA-BB-CC-DD-EE-FF",
            "\"bjut_wifi\"",
            "aa:bb:cc:dd:ee:ff"
        ));
        assert!(!same_exact_wifi_identity(
            "bjut_wifi",
            "AA-BB-CC-DD-EE-FF",
            "bjut-wifi",
            "aa:bb:cc:dd:ee:ff"
        ));
    }

    #[test]
    fn official_update_manifest_urls_are_tightly_scoped() {
        for allowed in [
            "https://github.com/key-zhzr/BJUT-Auto-Login/releases/latest/download/latest.json",
            "https://github.com/key-zhzr/BJUT-Auto-Login/releases/download/v0.1.5/latest.json",
        ] {
            assert!(is_official_update_manifest_endpoint(
                &reqwest::Url::parse(allowed).unwrap()
            ));
        }
        for rejected in [
            "http://github.com/key-zhzr/BJUT-Auto-Login/releases/latest/download/latest.json",
            "https://example.com/key-zhzr/BJUT-Auto-Login/releases/latest/download/latest.json",
            "https://github.com/key-zhzr/BJUT-Auto-Login/releases/download/v0.1.5/app.json",
            "https://github.com/key-zhzr/BJUT-Auto-Login/releases/latest/download/latest.json?x=1",
        ] {
            assert!(!is_official_update_manifest_endpoint(
                &reqwest::Url::parse(rejected).unwrap()
            ));
        }
        assert!(valid_update_manifest_version("0.1.5"));
        assert!(valid_update_manifest_version("0.1.6-beta.1"));
        assert!(valid_update_manifest_version("v1.2.3-rc.1+build.7"));
        assert!(!valid_update_manifest_version("../../latest"));
        assert!(!valid_update_manifest_version("1.2"));
        assert!(!valid_update_manifest_version("01.2.3"));
        assert!(!valid_update_manifest_version("1.2.3-beta.01"));
        assert!(!valid_update_manifest_version("1.2.3+"));
    }

    #[test]
    fn official_release_atom_parser_accepts_only_trusted_semver_links() {
        let atom = r#"
            <feed>
              <entry><link href="https://example.com/key-zhzr/BJUT-Auto-Login/releases/tag/v9.9.9"/></entry>
              <entry><link href="https://github.com/key-zhzr/BJUT-Auto-Login/releases/tag/v0.1.6-beta.2"/></entry>
              <entry><link href="https://github.com/key-zhzr/BJUT-Auto-Login/releases/tag/v0.1.5"/></entry>
            </feed>
        "#;
        assert_eq!(
            latest_official_release_tag_from_atom(atom).as_deref(),
            Some("v0.1.6-beta.2")
        );
        assert!(latest_official_release_tag_from_atom(
            r#"<link href="https://github.com/key-zhzr/BJUT-Auto-Login/releases/tag/../../latest"/>"#
        )
        .is_none());
    }

    #[test]
    fn mobile_data_detection_uses_battery_friendly_minimum_intervals() {
        assert_eq!(mobile_data_check_interval(15, false), 120);
        assert_eq!(mobile_data_check_interval(60, true), 300);
        assert_eq!(mobile_data_check_interval(600, false), 600);
        assert_eq!(mobile_data_check_interval(600, true), 600);
    }

    #[test]
    fn billing_account_selection_honors_requested_and_default_accounts() {
        let config: AppConfig = serde_json::from_value(serde_json::json!({
            "accounts": [
                {"user": "20260001", "pass": "first", "isDefault": true},
                {"user": "20260002", "pass": "second", "isDefault": false},
                {"user": "20260003", "pass": "disabled", "isDefault": false, "isDisabled": true}
            ]
        }))
        .unwrap();

        assert_eq!(
            selected_billing_account(&config, Some("20260002")).map(|account| account.user),
            Some("20260002".to_string())
        );
        assert_eq!(
            selected_billing_account(&config, None).map(|account| account.user),
            Some("20260001".to_string())
        );
        assert_eq!(
            selected_billing_account(&config, Some("20260003")).map(|account| account.user),
            Some("20260003".to_string())
        );
        assert!(!accounts_for_profile(config.accounts.clone(), None)
            .iter()
            .any(|account| account.user == "20260003"));
        assert!(selected_billing_account(&config, Some("missing")).is_none());
    }

    #[test]
    fn promoting_default_account_moves_it_to_the_front() {
        let mut accounts = vec![
            Account {
                user: "20260001".to_string(),
                pass: "first".to_string(),
                is_default: true,
                is_disabled: None,
            },
            Account {
                user: "20260002".to_string(),
                pass: "second".to_string(),
                is_default: false,
                is_disabled: None,
            },
            Account {
                user: "20260003".to_string(),
                pass: "third".to_string(),
                is_default: false,
                is_disabled: None,
            },
        ];

        assert_eq!(
            promote_default_account(&mut accounts, 2),
            Some("20260003".to_string())
        );
        assert_eq!(
            accounts
                .iter()
                .map(|account| account.user.as_str())
                .collect::<Vec<_>>(),
            vec!["20260003", "20260001", "20260002"]
        );
        assert_eq!(
            accounts
                .iter()
                .map(|account| account.is_default)
                .collect::<Vec<_>>(),
            vec![true, false, false]
        );
        assert_eq!(promote_default_account(&mut accounts, 9), None);
    }

    #[test]
    fn public_config_never_contains_passwords() {
        let persisted_session: campus_services::PersistedCampusSession =
            serde_json::from_value(serde_json::json!({
                "account": "20260001",
                "cookies": [{
                    "name": "eai-sess",
                    "value": "durable-cookie-secret",
                    "domain": "itsapp.bjut.edu.cn",
                    "host_only": true,
                    "path": "/",
                    "expires_at": 4102444800_i64,
                    "secure": true,
                    "http_only": true
                }],
                "saved_at": 1784707200_i64
            }))
            .unwrap();
        let config = AppConfig {
            preferred_interface: String::new(),
            adaptive_network_checks: true,
            accounts: vec![Account {
                user: "20260001".to_string(),
                pass: "secret".to_string(),
                is_default: true,
                is_disabled: None,
            }],
            auto_login: true,
            check_interval: 15,
            check_interval_bg: 60,
            wifi_change_detect: true,
            log_level: "info".to_string(),
            theme: default_theme(),
            accent_color: default_accent_color(),
            color_mode: default_color_mode(),
            macos_dock_visible: Some(false),
            vpn_compatibility: default_vpn_compatibility(),
            vpn_maximum_until: None,
            whitelist: vec!["campus|trusted".to_string()],
            blacklist: vec!["guest|blocked".to_string()],
            network_profiles: vec![],
            usage_alerts: true,
            balance_alert_threshold: default_balance_alert_threshold(),
            flow_alert_threshold: default_flow_alert_threshold(),
            android_notification_mode: default_android_notification_mode(),
            android_notify_network_status: true,
            android_notify_login_results: true,
            android_notify_background_errors: true,
            campus_service_sessions: vec![persisted_session],
            recharge_transactions: recharge_state::RechargeJournal::default(),
        };
        let serialized = serde_json::to_string(&public_config(&config)).unwrap();
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("eai-sess"));
        assert!(!serialized.contains("durable-cookie-secret"));
        assert_eq!(public_config(&config).accounts[0].pass, "");
        assert!(public_config(&config).campus_service_sessions.is_empty());
        assert!(public_config(&config).whitelist.is_empty());
        assert!(public_config(&config).blacklist.is_empty());
        assert!(public_config(&config).recharge_transactions.0.is_empty());
    }

    #[test]
    fn backup_scope_preserves_unselected_fields_and_current_payment_state() {
        let mut current: AppConfig = serde_json::from_value(serde_json::json!({
            "theme": "apple27", "auto_login": true,
            "accounts": [{ "user": "old", "pass": "old-secret", "isDefault": true }]
        }))
        .unwrap();
        current
            .recharge_transactions
            .upsert(recharge_state::RechargeTransaction::prepared(
                "current-payment".into(),
                "wechat",
                "old".into(),
                "old".into(),
                "1.00".into(),
                "0.00".into(),
                123,
            ));
        let imported: AppConfig = serde_json::from_value(serde_json::json!({
            "theme": "winui", "auto_login": false,
            "accounts": [{ "user": "new", "pass": "new-secret", "isDefault": true }]
        }))
        .unwrap();
        let settings = merge_backup_config(
            &current,
            imported.clone(),
            BackupScope {
                settings: true,
                accounts: false,
            },
        );
        assert_eq!(settings.accounts, current.accounts);
        assert_eq!(settings.theme, "winui");
        let accounts = merge_backup_config(
            &current,
            imported.clone(),
            BackupScope {
                settings: false,
                accounts: true,
            },
        );
        assert_eq!(accounts.accounts, imported.accounts);
        assert_eq!(accounts.theme, current.theme);
        assert!(accounts.auto_login);
        assert_eq!(
            accounts.recharge_transactions,
            current.recharge_transactions
        );
        assert!(accounts.campus_service_sessions.is_empty());
        assert!(BackupScope {
            settings: false,
            accounts: false
        }
        .validate()
        .is_err());
        // Older version-3 backups had no scope and must still import both parts.
        let legacy: ConfigBackupPlaintext = serde_json::from_value(serde_json::json!({
            "format":"BJUT-AL-CONFIG", "version":3, "config":imported
        }))
        .unwrap();
        assert!(legacy.scope.settings && legacy.scope.accounts);
    }

    #[test]
    fn encrypted_config_backup_round_trip_preserves_every_password() {
        let config: AppConfig = serde_json::from_value(serde_json::json!({
            "accounts": [
                {"user": "a", "pass": "secret-a", "isDefault": true},
                {"user": "b", "pass": "secret-b", "isDefault": false},
                {"user": "c", "pass": "secret-c", "isDefault": false}
            ]
        }))
        .unwrap();
        let plaintext = ConfigBackupPlaintext {
            scope: BackupScope::default(),
            format: "BJUT-AL-CONFIG".to_string(),
            version: CONFIG_BACKUP_VERSION,
            config,
            ui_preferences: serde_json::json!({"moreOptions": "false"}),
        };
        let payload = encrypt_config_backup_payload(&plaintext, b"backup-passphrase").unwrap();
        let restored = decrypt_config_backup_payload(&payload, b"backup-passphrase").unwrap();
        assert_eq!(restored.config.accounts.len(), 3);
        assert_eq!(restored.config.accounts[0].pass, "secret-a");
        assert_eq!(restored.config.accounts[1].pass, "secret-b");
        assert_eq!(restored.config.accounts[2].pass, "secret-c");
        assert!(decrypt_config_backup_payload(&payload, b"wrong-passphrase").is_err());
    }

    #[test]
    fn credential_fingerprint_matches_frontend_canonical_json() {
        let config: AppConfig = serde_json::from_value(serde_json::json!({
            "accounts": [
                {"user": "25000001", "pass": "secret", "isDefault": true}
            ]
        }))
        .unwrap();
        assert_eq!(
            credential_snapshot_fingerprint(&config, &["25000001".to_string()]).as_deref(),
            Some("RIaN0amLX2hGe8y+AWFlPi5loCz5luI2i2rvaEyF2m4=")
        );
        assert!(credential_snapshot_fingerprint(&config, &["missing".to_string()]).is_none());
    }

    #[test]
    fn maximum_vpn_mode_expires_to_high_compatibility() {
        let mut config: AppConfig = serde_json::from_value(serde_json::json!({
            "accounts": [],
            "vpn_compatibility": "maximum",
            "vpn_maximum_until": chrono::Utc::now().timestamp() - 1
        }))
        .unwrap();
        assert_eq!(effective_vpn_compatibility(&config), VpnCompatibility::High);

        config.vpn_maximum_until = Some(chrono::Utc::now().timestamp() + 60);
        assert_eq!(
            effective_vpn_compatibility(&config),
            VpnCompatibility::Maximum
        );
    }

    #[test]
    fn legacy_config_uses_compatible_android_notification_defaults() {
        let config: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[],
            "autoLogin":true
        }"#,
        )
        .unwrap();

        assert_eq!(config.android_notification_mode, "combined");
        assert!(config.android_notify_network_status);
        assert!(config.android_notify_login_results);
        assert!(config.android_notify_background_errors);
    }

    #[test]
    fn migrates_passwords_from_legacy_plaintext_config() {
        let mut current: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[{"user":"20260001","pass":"","isDefault":true}]
        }"#,
        )
        .unwrap();
        let legacy: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[{"username":"20260001","password":"legacy-secret","is_default":true}],
            "autoLogin":true,
            "checkInterval":30
        }"#,
        )
        .unwrap();

        assert!(merge_legacy_credentials(&mut current, &legacy));
        assert_eq!(current.accounts[0].pass, "legacy-secret");
        assert!(legacy.auto_login);
        assert_eq!(legacy.check_interval, 30);
    }

    #[test]
    fn legacy_migration_never_overwrites_a_new_password() {
        let mut current: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[{"user":"20260001","pass":"new-secret","isDefault":true}]
        }"#,
        )
        .unwrap();
        let legacy: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[{"user":"20260001","pass":"old-secret","isDefault":true}]
        }"#,
        )
        .unwrap();

        assert!(!merge_legacy_credentials(&mut current, &legacy));
        assert_eq!(current.accounts[0].pass, "new-secret");
    }

    #[test]
    fn legacy_migration_keeps_current_values_and_adds_missing_accounts() {
        let mut current: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[{"user":"a","pass":"new-a","isDefault":true}]
        }"#,
        )
        .unwrap();
        let legacy: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[
                {"user":"a","pass":"old-a","isDefault":true},
                {"user":"b","pass":"old-b","isDefault":true}
            ]
        }"#,
        )
        .unwrap();

        assert!(merge_legacy_credentials(&mut current, &legacy));
        assert_eq!(current.accounts.len(), 2);
        assert_eq!(current.accounts[0].pass, "new-a");
        assert_eq!(current.accounts[1].user, "b");
        assert_eq!(current.accounts[1].pass, "old-b");
        assert!(!current.accounts[1].is_default);
    }

    #[test]
    fn blank_password_updates_reuse_existing_secrets() {
        let existing: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[{"user":"a","pass":"saved-secret","isDefault":true}]
        }"#,
        )
        .unwrap();
        let mut update: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[{"user":"a","pass":"","isDefault":true}]
        }"#,
        )
        .unwrap();

        assert!(fill_missing_passwords(&mut update, &existing));
        assert_eq!(update.accounts[0].pass, "saved-secret");
    }

    #[test]
    fn partial_password_repair_keeps_other_unrecoverable_accounts_editable() {
        let existing: AppConfig = serde_json::from_str(
            r#"{
            "accounts":[
                {"user":"a","pass":"","isDefault":true},
                {"user":"b","pass":"","isDefault":false}
            ]
        }"#,
        )
        .unwrap();
        let mut update = existing.clone();
        update.accounts[0].pass = "re-entered-secret".to_string();

        assert!(!fill_missing_passwords(&mut update, &existing));
        assert_eq!(update.accounts[0].pass, "re-entered-secret");
        assert!(update.accounts[1].pass.is_empty());
    }

    #[test]
    fn request_errors_never_include_credential_urls() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime.block_on(async {
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(100))
                .use_rustls_tls()
                .build()
                .unwrap()
                .get("http://127.0.0.1:0/login?user=student&password=top-secret")
                .send()
                .await
                .unwrap_err()
        });

        let message = redact_request_error(error);
        assert!(!message.contains("student"));
        assert!(!message.contains("top-secret"));
        assert!(!message.contains("password="));
    }

    #[test]
    fn automatic_login_requires_a_recognized_or_explicitly_trusted_network() {
        let whitelist = vec!["custom-campus|10:20:30:40:50:60".to_string()];
        assert!(!is_known_campus_ssid("evil-bjut-wifi"));
        assert!(automatic_login_network_allowed(NetworkTrustInput {
            login_type: &LoginType::Type1,
            ssid: "bjut-sushe-5G-24vF",
            bssid: "fa:53:29:12:34:56",
            ip: "10.21.2.3",
            transport: "wifi",
            identity_fresh: true,
            whitelist: &[],
            blacklist: &[],
        })
        .is_ok());
        assert!(automatic_login_network_allowed(NetworkTrustInput {
            login_type: &LoginType::Type1,
            ssid: "evil-ap",
            bssid: "10:20:30:40:50:60",
            ip: "10.21.2.3",
            transport: "wifi",
            identity_fresh: true,
            whitelist: &[],
            blacklist: &[],
        })
        .is_err());
        assert!(automatic_login_network_allowed(NetworkTrustInput {
            login_type: &LoginType::Type2,
            ssid: "custom-campus",
            bssid: "10:20:30:40:50:60",
            ip: "10.21.2.3",
            transport: "wifi",
            identity_fresh: true,
            whitelist: &whitelist,
            blacklist: &[],
        })
        .is_ok());
        assert!(automatic_login_network_allowed(NetworkTrustInput {
            login_type: &LoginType::Type3,
            ssid: "",
            bssid: "",
            ip: "192.168.1.5",
            transport: "ethernet",
            identity_fresh: true,
            whitelist: &[],
            blacklist: &[],
        })
        .is_err());
    }

    #[test]
    fn account_failures_use_bounded_cooldowns() {
        assert_eq!(
            classify_account_failure("密码错误", 1),
            ("credential", 1800)
        );
        assert_eq!(classify_account_failure("余额不足", 1), ("balance", 21600));
        assert_eq!(
            classify_account_failure("请求出错: timeout", 1),
            ("network", 15)
        );
        assert_eq!(
            classify_account_failure("请求出错: timeout", 20),
            ("network", 900)
        );
        assert_eq!(
            classify_account_failure("认证服务器繁忙", 1),
            ("server", 60)
        );
        assert_eq!(
            classify_account_failure("认证服务器繁忙", 20),
            ("server", 900)
        );
    }

    #[test]
    fn account_health_view_reports_active_cooldown() {
        let mut health = HashMap::new();
        health.insert(
            "student".to_string(),
            AccountHealth {
                consecutive_failures: 1,
                cooldown_until: Some(chrono::Utc::now().timestamp() + 120),
                failure_kind: Some("credential".to_string()),
                ..Default::default()
            },
        );

        let views = account_health_views(&health);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].status, "needs_attention");
        assert!(views[0].cooldown_seconds > 0);
    }

    #[test]
    fn network_profiles_match_exact_network_and_order_accounts() {
        let config: AppConfig = serde_json::from_str(
            r#"{
            "accounts": [
                {"user":"first","pass":"one","isDefault":true},
                {"user":"second","pass":"two","isDefault":false}
            ],
            "network_profiles": [{
                "id":"dorm","name":"宿舍","ssid":"bjut_sushe","login_type":"bjut-sushe",
                "account_order":["second"],"enabled":true
            }]
        }"#,
        )
        .unwrap();
        let profile = matching_network_profile(&config, "BJUT_SUSHE", "", &LoginType::Type1)
            .expect("profile should match case-insensitively");
        assert_eq!(
            login_type_from_profile(&profile.login_type),
            Some(LoginType::Type1)
        );
        let ordered = accounts_for_profile(config.accounts, Some(&profile));
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].user, "second");
    }

    #[test]
    fn network_profile_controls_auto_login_per_authentication_type() {
        let config: AppConfig = serde_json::from_str(
            r#"{
            "network_profiles": [{
                "id":"mixed","name":"自动识别","ssid":"bjut_wifi","enabled":true,
                "auto_login":false,
                "auto_login_types":{"type1":true,"type2":false,"type3":true}
            }]
        }"#,
        )
        .unwrap();
        let profile = &config.network_profiles[0];
        assert!(profile_auto_login_enabled(
            Some(profile),
            &LoginType::Type1,
            false
        ));
        assert!(!profile_auto_login_enabled(
            Some(profile),
            &LoginType::Type2,
            true
        ));
        assert!(profile_auto_login_enabled(
            Some(profile),
            &LoginType::Type3,
            false
        ));
        assert!(!profile_auto_login_enabled(
            Some(profile),
            &LoginType::Unknown,
            true
        ));
    }

    #[test]
    fn campus_profile_names_map_to_the_documented_gateways() {
        assert_eq!(
            login_type_from_profile("bjut-sushe"),
            Some(LoginType::Type1)
        );
        assert_eq!(
            login_type_from_profile("bjut_sushe"),
            Some(LoginType::Type1)
        );
        assert_eq!(login_type_from_profile("bjut-wifi"), Some(LoginType::Type2));
        assert_eq!(login_type_from_profile("bjut_wifi"), Some(LoginType::Type2));
        assert_eq!(login_type_from_profile("wired"), Some(LoginType::Type3));
        assert_eq!(login_type_from_profile("lgn-wired"), Some(LoginType::Type3));
    }

    #[test]
    fn vpn_compatibility_selects_secure_or_direct_probe_endpoints() {
        let dorm = portal_probe_urls(VpnCompatibility::Minimum, &LoginType::Type1);
        assert_eq!(dorm.len(), 2);
        assert!(dorm.iter().all(|url| {
            let url = reqwest::Url::parse(url).unwrap();
            url.scheme() == "https"
                && url.port() == Some(802)
                && url
                    .host_str()
                    .is_some_and(|host| host == WLGN_HOST || host == LGN_HOST)
                && url.path() == "/eportal/portal/page/loadConfig"
        }));
        let wifi = portal_probe_urls(VpnCompatibility::High, &LoginType::Type2);
        assert_eq!(wifi.len(), 1);
        assert!(wifi[0].starts_with("https://wlgn.bjut.edu.cn/drcom/chkstatus?"));
        let wifi_direct = portal_probe_urls(VpnCompatibility::Maximum, &LoginType::Type2);
        assert_eq!(wifi_direct.len(), 1);
        assert!(wifi_direct[0].starts_with("http://10.21.251.3/drcom/chkstatus?"));
        let wired = portal_probe_urls(VpnCompatibility::Maximum, &LoginType::Type3);
        assert_eq!(wired.len(), 2);
        assert!(wired[0].starts_with("http://172.30.201.2:801/eportal/portal/page/loadUserInfo?"));
        assert!(wired[1].starts_with("http://172.30.201.10:801/eportal/portal/page/loadUserInfo?"));
    }

    #[test]
    fn dashboard_user_info_uses_the_correct_protocol_for_vpn_mode() {
        let url = reqwest::Url::parse(&lgn_user_info_url(VpnCompatibility::Minimum)).unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("lgn.bjut.edu.cn"));
        assert_eq!(url.port(), Some(802));

        let direct = reqwest::Url::parse(&lgn_user_info_url(VpnCompatibility::Maximum)).unwrap();
        assert_eq!(direct.scheme(), "http");
        assert_eq!(direct.host_str(), Some("172.30.201.2"));
        assert_eq!(direct.port(), Some(801));
    }

    #[test]
    fn lgn_protocol_is_wired_only_unless_explicitly_trusted() {
        assert!(automatic_login_network_allowed(NetworkTrustInput {
            login_type: &LoginType::Type3,
            ssid: "bjut_wifi",
            bssid: "10:20:30:40:50:60",
            ip: "10.21.2.3",
            transport: "wifi",
            identity_fresh: true,
            whitelist: &[],
            blacklist: &[],
        })
        .is_err());
        assert!(automatic_login_network_allowed(NetworkTrustInput {
            login_type: &LoginType::Type3,
            ssid: "",
            bssid: "",
            ip: "10.21.2.3",
            transport: "ethernet",
            identity_fresh: true,
            whitelist: &[],
            blacklist: &[],
        })
        .is_ok());
        assert!(automatic_login_network_allowed(NetworkTrustInput {
            login_type: &LoginType::Type3,
            ssid: "",
            bssid: "",
            ip: "10.21.2.3",
            transport: "wifi",
            identity_fresh: true,
            whitelist: &[],
            blacklist: &[],
        })
        .is_err());
    }

    #[test]
    fn usage_numbers_are_parsed_from_display_values() {
        assert_eq!(first_decimal("余额 9.50 元"), Some(9.5));
        assert_eq!(first_decimal("4.25 GB"), Some(4.25));
        assert_eq!(first_decimal("无限"), None);
    }

    #[test]
    fn campus_recharge_hours_use_beijing_half_open_range() {
        assert!(!campus_recharge_open_at_hour(5));
        assert!(campus_recharge_open_at_hour(6));
        assert!(campus_recharge_open_at_hour(22));
        assert!(!campus_recharge_open_at_hour(23));
    }

    #[test]
    fn update_download_allowlist_rejects_url_ambiguity() {
        let valid = reqwest::Url::parse(
            "https://github.com/key-zhzr/BJUT-Auto-Login/releases/download/v0.1.5/latest.json",
        )
        .unwrap();
        assert!(is_official_github_release_download(&valid));

        for invalid in [
            "http://github.com/key-zhzr/BJUT-Auto-Login/releases/download/v0.1.5/latest.json",
            "https://example.com/key-zhzr/BJUT-Auto-Login/releases/download/v0.1.5/latest.json",
            "https://user@github.com/key-zhzr/BJUT-Auto-Login/releases/download/v0.1.5/latest.json",
            "https://github.com:444/key-zhzr/BJUT-Auto-Login/releases/download/v0.1.5/latest.json",
            "https://github.com/key-zhzr/BJUT-Auto-Login/releases/download/v0.1.5/latest.json?asset=other",
            "https://github.com/key-zhzr/BJUT-Auto-Login/releases/download/v0.1.5/latest.json#other",
        ] {
            assert!(
                !is_official_github_release_download(
                    &reqwest::Url::parse(invalid).expect("test URL is valid")
                ),
                "{invalid} should be rejected"
            );
        }
    }
}
