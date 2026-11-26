//! Read-only DNS checks on the system route, including split-horizon campus records.
use crate::{
    campus_dns, make_diagnostic_step,
    probe_transport::{self, Ipv6Resolution, IPV6_PROBE_HOST},
    DiagnosticStep,
};
use std::{net::IpAddr, time::Instant};
use tokio::time::{timeout_at, Instant as Deadline};

const PUBLIC_HOST: &str = "www.baidu.com";
const BILLING_HOST: &str = "jfself.bjut.edu.cn";

pub(crate) async fn diagnose(
    network: &serde_json::Value,
    campus_link: bool,
    deadline: Deadline,
) -> DiagnosticStep {
    let started = Instant::now();
    let (public, ipv6, billing, gateway, servers) = futures_util::join!(
        lookup(network, PUBLIC_HOST, deadline),
        timeout_at(
            deadline,
            probe_transport::resolve_ipv6(network, false, IPV6_PROBE_HOST)
        ),
        lookup(network, BILLING_HOST, deadline),
        lookup(network, crate::LGN_HOST, deadline),
        timeout_at(deadline, system_resolvers(network)),
    );
    let (ipv6_ok, public_ipv6_ok, ipv6_detail) = match ipv6 {
        Ok(Ipv6Resolution { configured, public }) => {
            let ok = configured.is_ok();
            let mut detail = format!(
                "{IPV6_PROBE_HOST}（AAAA，系统 DNS）：{}",
                lookup_detail(&configured.map_err(|error| error.detail))
            );
            let public_ok = public.as_ref().is_some_and(Result::is_ok);
            if let Some(result) = public {
                detail.push_str(&format!(
                    "\n{IPV6_PROBE_HOST}（AAAA，公共 DNS 回退）：{}",
                    lookup_detail(&result.map_err(|error| error.detail))
                ));
            }
            (ok, public_ok, detail)
        }
        Err(_) => (false, false, format!("{IPV6_PROBE_HOST}（AAAA）：诊断超时")),
    };
    let public_ok = public.is_ok() || ipv6_ok;
    let ipv6_expected = network["ipv6"].as_array().is_some_and(|addresses| {
        addresses
            .iter()
            .filter_map(serde_json::Value::as_str)
            .any(crate::network_inventory::usable_ipv6)
    });
    let ipv6_note = if public_ipv6_ok {
        " · IPv6 使用公共 DNS 回退"
    } else if ipv6_expected && !ipv6_ok {
        " · IPv6 解析异常"
    } else {
        ""
    };
    let private_billing = billing.as_ref().is_ok_and(|addresses| {
        addresses
            .iter()
            .any(|address| matches!(address, IpAddr::V4(ip) if ip.is_private()))
    });
    let mut detail = format!(
        "{PUBLIC_HOST}（A）：{}\n{ipv6_detail}\n{BILLING_HOST}（校园内网，A）：{}\n{}（校园认证，A）：{}",
        lookup_detail(&public),
        if billing.is_ok() && !private_billing {
            format!("返回地址，但未取得计费系统的内网地址（{}）", lookup_detail(&billing))
        } else {
            lookup_detail(&billing)
        },
        crate::LGN_HOST,
        lookup_detail(&gateway),
    );
    let servers = servers.unwrap_or_default();
    if servers.is_empty() {
        detail.push_str("\n系统 DNS 配置：未能读取解析器地址");
    } else {
        detail.push_str("\n系统 DNS 配置（含分域或分接口项）：");
        for address in servers {
            let assessment = if campus_dns::is_known_server(address) {
                "已知校园 DNS"
            } else if address.is_loopback() {
                "本地解析代理"
            } else {
                "未匹配已知校园 DNS"
            };
            detail.push_str(&format!("\n{address} · {assessment}"));
        }
    }
    make_diagnostic_step(
        "dns",
        "系统 DNS",
        started,
        if !public_ok || (ipv6_expected && !ipv6_ok) || (campus_link && !private_billing) {
            "warning"
        } else {
            "success"
        },
        format!(
            "{} · {}{ipv6_note}",
            if public_ok {
                "公网可解析"
            } else {
                "公网解析失败"
            },
            if private_billing {
                "校内域名可解析"
            } else {
                "未取得校内地址"
            }
        ),
        detail,
    )
}

async fn lookup(
    network: &serde_json::Value,
    host: &str,
    deadline: Deadline,
) -> Result<Vec<IpAddr>, String> {
    timeout_at(
        deadline,
        probe_transport::resolve_system(network, false, host),
    )
    .await
    .map_err(|_| "诊断超时".to_string())?
    .map_err(|error| error.detail)
}

fn lookup_detail(result: &Result<Vec<IpAddr>, String>) -> String {
    match result {
        Ok(addresses) => format!(
            "解析成功 · {}",
            addresses
                .iter()
                .take(4)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" / ")
        ),
        Err(error) => format!("解析失败 · {error}"),
    }
}

async fn system_resolvers(_network: &serde_json::Value) -> Vec<IpAddr> {
    #[cfg(target_os = "android")]
    let servers = _network["defaultDnsServers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| parse_resolver(value.as_str()?))
        .collect();
    #[cfg(not(target_os = "android"))]
    let servers = tokio::task::spawn_blocking(read_system_resolvers)
        .await
        .unwrap_or_default();
    let mut servers: Vec<IpAddr> = servers;
    servers.sort_unstable();
    servers.dedup();
    servers
}

fn parse_resolver(value: &str) -> Option<IpAddr> {
    value.trim().split('%').next()?.parse().ok()
}

#[cfg(target_os = "macos")]
fn read_system_resolvers() -> Vec<IpAddr> {
    crate::network_platform::macos_read_command("/usr/sbin/scutil", &["--dns"])
        .lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once(':')?;
            key.starts_with("nameserver[")
                .then(|| parse_resolver(value))
                .flatten()
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn read_system_resolvers() -> Vec<IpAddr> {
    crate::network_inventory::adapters()
        .into_iter()
        .filter(|adapter| adapter.connected)
        .flat_map(|adapter| {
            adapter
                .dns_servers
                .into_iter()
                .filter_map(|address| parse_resolver(&address))
        })
        .collect()
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
fn read_system_resolvers() -> Vec<IpAddr> {
    let mut addresses = read_resolv_conf("/etc/resolv.conf");
    if addresses.iter().any(
        |address| matches!(address, IpAddr::V4(ip) if matches!(ip.octets(), [127, 0, 0, 53 | 54])),
    ) {
        addresses.extend(read_resolv_conf("/run/systemd/resolve/resolv.conf"));
    }
    addresses
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
fn read_resolv_conf(path: &str) -> Vec<IpAddr> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next()? == "nameserver")
                .then(|| parse_resolver(fields.next()?))
                .flatten()
        })
        .collect()
}
