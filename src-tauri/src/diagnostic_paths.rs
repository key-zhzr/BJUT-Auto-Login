//! Presentation for the shared system and physical connectivity results.
use crate::dual_stack::{DualStackReport, FamilyConnectivity};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectivityCell {
    status: String,
    duration_ms: u128,
}

impl From<&FamilyConnectivity> for ConnectivityCell {
    fn from(value: &FamilyConnectivity) -> Self {
        Self {
            status: value.status.clone(),
            duration_ms: value.duration_ms,
        }
    }
}

#[derive(serde::Serialize)]
pub(crate) struct ConnectivityMatrix {
    system: [ConnectivityCell; 2],
    direct: [ConnectivityCell; 2],
}

#[derive(Clone)]
pub(crate) struct Paths {
    pub(crate) system: DualStackReport,
    pub(crate) direct: DualStackReport,
}

impl Paths {
    pub(crate) fn pending() -> Self {
        let mut system = crate::dual_stack::unavailable("正在检测");
        system.ipv4.status = "checking".into();
        system.ipv6.status = "checking".into();
        let mut direct = system.clone();
        direct.scope = "physical".into();
        Self { system, direct }
    }

    pub(crate) fn finish(&mut self, direct_supported: bool, timed_out_ms: Option<u128>) {
        for (report, supported) in [
            (&mut self.system, true),
            (&mut self.direct, direct_supported),
        ] {
            for result in [&mut report.ipv4, &mut report.ipv6] {
                if result.status != "checking" {
                    continue;
                }
                if let Some(duration_ms) = timed_out_ms.filter(|_| supported) {
                    result.status = "timeout".into();
                    result.detail = "已达到本次诊断的 2 秒总预算".into();
                    result.duration_ms = duration_ms;
                } else {
                    result.status = "unavailable".into();
                    result.detail = if supported {
                        "未取得此路径的结果"
                    } else {
                        "当前没有可用于直连检测的 Wi-Fi 或有线网卡"
                    }
                    .into();
                }
            }
        }
    }

    pub(crate) fn step(&self) -> crate::DiagnosticStep {
        let mut lines = Vec::new();
        let mut pending = false;
        let mut online = false;
        let mut duration_ms = 0;
        for (path, report) in [
            ("当前网络（含 VPN）", &self.system),
            ("网卡直连（绕过 VPN）", &self.direct),
        ] {
            for (family, result) in [("IPv4", &report.ipv4), ("IPv6", &report.ipv6)] {
                pending |= result.status == "checking";
                online |= result.status == "reachable";
                duration_ms = duration_ms.max(result.duration_ms);
                let status = match result.status.as_str() {
                    "checking" => "检测中",
                    "reachable" => "通过",
                    "timeout" => "超时",
                    "not_configured" => "未配置",
                    "unavailable" => "无法探测",
                    _ => "未通过",
                };
                let detail = if family == "IPv6" && result.status == "dns_error" {
                    "连通性未验证，IPv6 目标的解析结果见「系统 DNS」"
                } else {
                    &result.detail
                };
                let timing = if result.duration_ms == 0 {
                    String::new()
                } else {
                    format!("（{} ms）", result.duration_ms)
                };
                lines.push(format!("{path} · {family}：{status}{timing} · {detail}"));
            }
        }
        crate::DiagnosticStep {
            id: "internet".into(),
            label: "互联网连通性".into(),
            status: if pending {
                "checking"
            } else if online {
                "success"
            } else {
                "warning"
            }
            .into(),
            summary: if pending {
                "正在检测"
            } else if online {
                "互联网可用"
            } else {
                "互联网连通性未确认"
            }
            .into(),
            message: lines.join("\n"),
            duration_ms,
            connectivity: Some(ConnectivityMatrix {
                system: [&self.system.ipv4, &self.system.ipv6].map(ConnectivityCell::from),
                direct: [&self.direct.ipv4, &self.direct.ipv6].map(ConnectivityCell::from),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finishing_at_the_deadline_preserves_completed_results() {
        let mut paths = Paths::pending();
        paths.system.ipv4.status = "reachable".into();
        paths.system.ipv4.duration_ms = 43;
        paths.finish(false, Some(1900));
        let step = paths.step();
        let matrix = step.connectivity.unwrap();
        assert_eq!(matrix.system[0].status, "reachable");
        assert_eq!(matrix.system[0].duration_ms, 43);
        assert_eq!(matrix.system[1].status, "timeout");
        assert_eq!(matrix.system[1].duration_ms, 1900);
        assert_eq!(matrix.direct[0].status, "unavailable");
        assert_eq!(matrix.direct[1].status, "unavailable");
    }
}
