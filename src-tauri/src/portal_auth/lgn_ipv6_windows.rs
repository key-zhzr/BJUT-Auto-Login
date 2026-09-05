//! Windows reqwest has no interface binding option. Select an IPv6 source
//! from the adapter that owns the already verified physical IPv4 instead.

use std::mem::size_of;
use std::net::{Ipv4Addr, Ipv6Addr};
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
    IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::Networking::WinSock::{
    IpDadStatePreferred, AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR_IN, SOCKADDR_IN6,
};

pub(super) fn source_ipv6(physical_ipv4: Ipv4Addr) -> Result<Ipv6Addr, String> {
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut byte_count = 0u32;
    // SAFETY: the first call only requests the required buffer size.
    let result =
        unsafe { GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, None, &mut byte_count) };
    if result != ERROR_BUFFER_OVERFLOW.0 || byte_count == 0 {
        return Err("无法读取校园网接口的 IPv6 地址".to_string());
    }

    // The adapter list may change between calls. Retry with the new size and
    // use u64 storage to satisfy the alignment of the Windows record unions.
    for _ in 0..3 {
        let mut storage = vec![0u64; (byte_count as usize).div_ceil(size_of::<u64>())];
        let first = storage.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        // SAFETY: storage is aligned and contains at least byte_count bytes.
        let result = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC.0 as u32,
                flags,
                None,
                Some(first),
                &mut byte_count,
            )
        };
        if result == ERROR_BUFFER_OVERFLOW.0 {
            continue;
        }
        if result != NO_ERROR.0 {
            return Err("无法读取校园网接口的 IPv6 地址".to_string());
        }

        let mut adapter_ptr = first;
        while !adapter_ptr.is_null() {
            // SAFETY: all linked adapter/address records are owned by storage,
            // which remains alive throughout traversal.
            let adapter = unsafe { &*adapter_ptr };
            let mut owns_ipv4 = false;
            let mut ipv6 = None;
            let mut address_ptr = adapter.FirstUnicastAddress;
            while !address_ptr.is_null() {
                // SAFETY: address_ptr is a record in the returned linked list.
                let address = unsafe { &*address_ptr };
                let socket = address.Address;
                if !socket.lpSockaddr.is_null()
                    && socket.iSockaddrLength as usize >= size_of::<u16>()
                {
                    // SAFETY: the sockaddr contains the family field; full
                    // family-specific lengths are checked before each cast.
                    let family = unsafe { (*socket.lpSockaddr).sa_family };
                    if family == AF_INET
                        && socket.iSockaddrLength as usize >= size_of::<SOCKADDR_IN>()
                    {
                        // SAFETY: family and record length match SOCKADDR_IN.
                        let addr = unsafe { &*socket.lpSockaddr.cast::<SOCKADDR_IN>() };
                        // SAFETY: S_un_b is the byte view of the IPv4 union.
                        let bytes = unsafe { addr.sin_addr.S_un.S_un_b };
                        owns_ipv4 |= Ipv4Addr::new(bytes.s_b1, bytes.s_b2, bytes.s_b3, bytes.s_b4)
                            == physical_ipv4;
                    } else if family == AF_INET6
                        && socket.iSockaddrLength as usize >= size_of::<SOCKADDR_IN6>()
                        && address.DadState == IpDadStatePreferred
                        && address.PreferredLifetime > 0
                    {
                        // SAFETY: family and record length match SOCKADDR_IN6;
                        // Byte is the network-order byte view of the IPv6 union.
                        let addr = unsafe { &*socket.lpSockaddr.cast::<SOCKADDR_IN6>() };
                        let candidate = Ipv6Addr::from(unsafe { addr.sin6_addr.u.Byte });
                        if super::is_bjut_client_ipv6(&candidate) {
                            ipv6.get_or_insert(candidate);
                        }
                    }
                }
                address_ptr = address.Next;
            }
            if owns_ipv4 {
                return ipv6
                    .ok_or_else(|| "校园网 IPv4 所在接口没有可用的 BJUT IPv6 地址".to_string());
            }
            adapter_ptr = adapter.Next;
        }
        return Err("未找到校园网 IPv4 所在的物理接口".to_string());
    }
    Err("读取 IPv6 地址时网络接口发生变化，请重试".to_string())
}
