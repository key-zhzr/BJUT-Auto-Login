//! Presentation for the shared system and physical connectivity results.
use crate::dual_stack::DualStackReport;
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
                lines.push(format!(
                    "{path} · {family}：{status}（{} ms）{}",
                    result.duration_ms,
                    if matches!(
                        result.status.as_str(),
                        "checking" | "reachable" | "not_configured"
                    ) {
                        String::new()
                    } else {
                        format!(" · {}", result.detail)
                    }
                ));
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
            message: lines.join("\n"),
            duration_ms,
        }
    }
}
