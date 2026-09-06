use super::NetworkAdapter;
use std::mem::size_of;
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
    IF_TYPE_ETHERNET_CSMACD, IF_TYPE_IEEE80211, IF_TYPE_SOFTWARE_LOOPBACK, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::NetworkManagement::Ndis::{IfOperStatusUnknown, IfOperStatusUp};
use windows::Win32::Networking::WinSock::{
    IpDadStatePreferred, AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR_IN, SOCKADDR_IN6,
};

pub(super) fn adapters() -> Vec<NetworkAdapter> {
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut bytes = 0;
    // SAFETY: size query does not dereference an adapter buffer.
    if unsafe { GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, None, &mut bytes) }
        != ERROR_BUFFER_OVERFLOW.0
    {
        return Vec::new();
    }
    for _ in 0..3 {
        let mut storage = vec![0u64; (bytes as usize).div_ceil(size_of::<u64>())];
        let first = storage.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        // SAFETY: aligned storage remains alive throughout linked-list traversal.
        let result = unsafe {
            GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(first), &mut bytes)
        };
        if result == ERROR_BUFFER_OVERFLOW.0 {
            continue;
        }
        if result != NO_ERROR.0 {
            return Vec::new();
        }
        let mut entries = Vec::new();
        let mut ptr = first;
        while !ptr.is_null() {
            // SAFETY: all adapter/address pointers are owned by storage.
            let record = unsafe { &*ptr };
            ptr = record.Next;
            if record.IfType == IF_TYPE_SOFTWARE_LOOPBACK {
                continue;
            }
            let id = crate::normalize_windows_guid(
                &unsafe { record.AdapterName.to_string() }.unwrap_or_default(),
            );
            let mut name = unsafe { record.FriendlyName.to_string() }.unwrap_or_default();
            let description = unsafe { record.Description.to_string() }.unwrap_or_default();
            if name.trim().is_empty() {
                name.clone_from(&description);
            }
            if name.trim().is_empty() {
                name.clone_from(&id);
            }
            let index = unsafe { record.Anonymous1.Anonymous.IfIndex };
            let len = (record.PhysicalAddressLength as usize).min(record.PhysicalAddress.len());
            let physical = (crate::windows_interface_is_hardware(index)
                || (len >= 6 && record.PhysicalAddress[..len].iter().any(|byte| *byte != 0)))
                && !crate::windows_adapter_looks_virtual(&name, &description)
                && [IF_TYPE_ETHERNET_CSMACD, IF_TYPE_IEEE80211].contains(&record.IfType);
            let mut adapter = NetworkAdapter {
                id,
                interface_name: name.clone(),
                name,
                transport: if !physical {
                    "vpn"
                } else if record.IfType == IF_TYPE_IEEE80211 {
                    "wifi"
                } else {
                    "ethernet"
                }
                .to_string(),
                connected: record.OperStatus == IfOperStatusUp,
                selectable: physical,
                ..Default::default()
            };
            let mut address_ptr = record.FirstUnicastAddress;
            while !address_ptr.is_null() {
                let address = unsafe { &*address_ptr };
                address_ptr = address.Next;
                let socket = address.Address;
                if socket.lpSockaddr.is_null()
                    || (socket.iSockaddrLength as usize) < size_of::<u16>()
                {
                    continue;
                }
                let family = unsafe { (*socket.lpSockaddr).sa_family };
                if family == AF_INET && socket.iSockaddrLength as usize >= size_of::<SOCKADDR_IN>()
                {
                    let address = unsafe { &*socket.lpSockaddr.cast::<SOCKADDR_IN>() };
                    let bytes = unsafe { address.sin_addr.S_un.S_un_b };
                    let ip =
                        std::net::Ipv4Addr::new(bytes.s_b1, bytes.s_b2, bytes.s_b3, bytes.s_b4)
                            .to_string();
                    if crate::usable_physical_ipv4(&ip).is_some() {
                        adapter.ipv4.push(ip);
                    }
                } else if family == AF_INET6
                    && socket.iSockaddrLength as usize >= size_of::<SOCKADDR_IN6>()
                    && address.DadState == IpDadStatePreferred
                    && address.PreferredLifetime > 0
                {
                    let address = unsafe { &*socket.lpSockaddr.cast::<SOCKADDR_IN6>() };
                    let ip =
                        std::net::Ipv6Addr::from(unsafe { address.sin6_addr.u.Byte }).to_string();
                    if super::usable_ipv6(&ip) {
                        adapter.ipv6.push(ip);
                    }
                }
            }
            adapter.connected |= record.OperStatus == IfOperStatusUnknown
                && (!adapter.ipv4.is_empty() || !adapter.ipv6.is_empty());
            entries.push(adapter);
        }
        return entries;
    }
    Vec::new()
}
