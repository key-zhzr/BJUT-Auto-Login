//! Compose read-only diagnostics and export a redacted support bundle.
use super::*;
use crate::network_probe::DIAGNOSTIC_BUDGET;
use tokio::time::{timeout_at, Instant as Deadline};

fn emit_step(app: &tauri::AppHandle, run_id: &Option<String>, step: &DiagnosticStep) {
    if let Some(run_id) = run_id {
        let _ = app.emit(
            "network-diagnostic-step",
            serde_json::json!({ "runId": run_id, "step": step }),
        );
    }
}

#[tauri::command]
pub(super) async fn run_network_diagnostics(
    app: tauri::AppHandle,
    run_id: Option<String>,
) -> DiagnosticReport {
    let deadline = Deadline::now() + DIAGNOSTIC_BUDGET;
    let generation = app
        .state::<Arc<AppState>>()
        .network_change_generation
        .load(Ordering::SeqCst);
    emit_network_diagnostic_progress(&app, &run_id, 2, "正在读取当前网络接口…");
    let compatibility = app
        .try_state::<Arc<AppState>>()
        .map(|state| {
            let config = state.config.read().unwrap();
            effective_vpn_compatibility(&config)
        })
        .unwrap_or(VpnCompatibility::High);
    let mut steps = Vec::new();
    let mut push_step = |step: DiagnosticStep| {
        emit_step(&app, &run_id, &step);
        steps.push(step);
    };
    let identity_started = std::time::Instant::now();
    let prepared = timeout_at(
        deadline,
        tokio::task::spawn_blocking({
            let app = app.clone();
            move || {
                let network = get_network_info(app, Some(true));
                if Deadline::now() >= deadline {
                    return None;
                }
                #[cfg(target_os = "android")]
                let route_guard = AndroidWifiRouteGuard::bind_if_required(&network);
                #[cfg(not(target_os = "android"))]
                let route_guard = ();
                Some((network, route_guard))
            }
        }),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .flatten();
    let network = prepared
        .as_ref()
        .map(|(network, _)| network.clone())
        .unwrap_or_default();
    #[cfg(target_os = "android")]
    let wifi_route_failed = prepared.as_ref().is_some_and(|(_, guard)| guard.failed());
    #[cfg(not(target_os = "android"))]
    let wifi_route_failed = false;
    let transport = network_transport(&network).to_string();
    let mobile_data = is_mobile_data_network(&network);
    let ssid = network
        .get("ssid")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let bssid = network
        .get("bssid")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let identity_fresh = network_identity_is_fresh(&network);
    let interface_name = network
        .get("interfaceName")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let identity_source = network
        .get("identitySource")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let wifi_identity_error = network
        .get("wifiIdentityError")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let route_ip = network
        .get("routeIp")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let ip = network
        .get("ip")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let address = if ip.is_empty() {
        network["ipv6"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .find(|address| network_inventory::usable_ipv6(address))
            .unwrap_or_default()
    } else {
        &ip
    };
    let identity_status = if address.is_empty() {
        "error"
    } else if !wifi_identity_error.is_empty() {
        "warning"
    } else if mobile_data || !ssid.is_empty() || transport.eq_ignore_ascii_case("ethernet") {
        "success"
    } else {
        "warning"
    };
    let identity_message = if prepared.is_none() {
        "未在本次 2 秒预算内读到网卡信息".to_string()
    } else if address.is_empty() {
        "未检测到可用网络接口或 IP 地址".to_string()
    } else if mobile_data {
        format!("当前通过移动数据上网，本地 IP {address}")
    } else if wifi_identity_error == "locationPermissionDenied" {
        format!(
            "已取得接口 {} 的本地 IP {address}，但 Windows 未授予精确位置权限，无法读取 SSID/BSSID；请在系统“隐私和安全性 → 位置”中允许桌面应用访问位置",
            if interface_name.is_empty() {
                "未知"
            } else {
                interface_name
            }
        )
    } else if wifi_identity_error == "locationPermissionOrServiceUnavailable" {
        format!(
            "已取得接口 {} 的本地 IP {address}，但 Android 未返回 SSID/BSSID；请确认附近 Wi-Fi、前台位置权限和系统位置服务均已开启",
            if interface_name.is_empty() {
                "未知"
            } else {
                interface_name
            }
        )
    } else if transport.eq_ignore_ascii_case("ethernet") {
        format!(
            "已连接有线接口 {}，本地 IP {address}（身份来源：{identity_source}）",
            if interface_name.is_empty() {
                "未知"
            } else {
                interface_name
            }
        )
    } else if ssid.is_empty() || ssid.eq_ignore_ascii_case("<unknown ssid>") {
        format!("已取得本地 IP {address}，但未取得无线网络名称（可能为有线网络或权限不足）")
    } else {
        format!(
            "已连接 {ssid}，接口 {}，本地 IP {address}（身份来源：{identity_source}）",
            if interface_name.is_empty() {
                "未知"
            } else {
                interface_name
            }
        )
    };
    push_step(make_diagnostic_step(
        "network_identity",
        "网络接口",
        identity_started,
        identity_status,
        match identity_status {
            "success" if mobile_data => "移动数据已连接".to_string(),
            "success" if !ssid.is_empty() => format!("已连接 {ssid}"),
            "success" => format!("已连接 {interface_name}"),
            "warning" => "网卡已连接，部分身份信息缺失".to_string(),
            _ if prepared.is_none() => "读取超时".to_string(),
            _ => "未取得可用网络地址".to_string(),
        },
        identity_message,
    ));
    emit_network_diagnostic_progress(&app, &run_id, 16, "网络接口信息读取完成");

    let route_started = std::time::Instant::now();
    emit_network_diagnostic_progress(&app, &run_id, 20, "正在核对校园认证目标路由…");
    let route_is_same =
        !ip.is_empty() && route_ip == ip && usable_physical_ipv4(route_ip).is_some();
    push_step(make_diagnostic_step(
        "campus_route",
        "校园目标路由",
        route_started,
        if wifi_route_failed {
            "error"
        } else if route_is_same {
            "success"
        } else if ip.is_empty() && !address.is_empty() {
            "skipped"
        } else {
            "warning"
        },
        if wifi_route_failed {
            "绑定校园网卡失败"
        } else if route_is_same {
            "认证路由与认证网卡一致"
        } else if ip.is_empty() && !address.is_empty() {
            "当前使用 IPv6"
        } else if route_ip.is_empty() {
            "未取得认证路由"
        } else {
            "认证路由需核对"
        },
        if wifi_route_failed {
            "检测到非默认的待认证 Wi-Fi，但 Android 无法将请求绑定到该网络".to_string()
        } else if ip.is_empty() && !address.is_empty() {
            "当前网卡仅取得 IPv6，源 IPv4 对照不适用。IPv6 认证可用性请查看认证网关结果。"
                .to_string()
        } else if route_ip.is_empty() {
            "无法取得访问校园认证目标时系统选择的源 IPv4".to_string()
        } else if usable_physical_ipv4(route_ip).is_none() {
            format!("校园认证目标当前走了 VPN 虚拟地址（{route_ip}），请在 VPN 中放行校园网段")
        } else if ip.is_empty() {
            format!(
                "系统路由使用 {route_ip}，但未取得对应物理接口及 IPv4；已禁止不绑定接口的认证请求"
            )
        } else if route_ip == ip {
            format!("校园认证目标与当前网络身份使用同一源 IP：{route_ip}")
        } else {
            format!(
                "访问校园认证目标将使用 {route_ip}，与物理网络 IP {ip} 不同；可能存在 VPN 或独立路由"
            )
        },
    ));
    emit_network_diagnostic_progress(&app, &run_id, 30, "校园认证目标路由核对完成");

    let campus_started = std::time::Instant::now();
    emit_network_diagnostic_progress(&app, &run_id, 34, "正在检查校园网环境特征…");
    let campus_ip = is_campus_local_ip(&ip) || !network_campus_ipv6(&network).is_empty();
    let campus_ssid = is_known_campus_ssid(&ssid);
    let lgn_wired_hint = network
        .get("lgnWiredHint")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let lgn_wired_features = network
        .get("lgnWiredFeatures")
        .and_then(serde_json::Value::as_array)
        .map(|features| {
            features
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join("、")
        })
        .unwrap_or_default();
    let campus_status = if mobile_data {
        "skipped"
    } else if campus_ip && (campus_ssid || ssid.is_empty() || ssid.contains("unknown")) {
        "success"
    } else if campus_ip || campus_ssid {
        "warning"
    } else {
        "error"
    };
    let campus_message = format!(
        "传输类型：{}\n本地网段：{}\nlgn 有线特征：{}\nSSID 特征：{}\nWi-Fi BSSID：{}\n网卡信息已同步：{}\n校园目标路由绑定：{}",
        if transport.is_empty() { "未知" } else { &transport },
        if campus_ip { "符合" } else { "不符合" },
        if lgn_wired_hint {
            if lgn_wired_features.is_empty() {
                "符合（物理以太网 + 172.26/16；仍需 lgn 响应确认）".to_string()
            } else {
                format!("符合（{lgn_wired_features}；仍需 lgn 响应确认）")
            }
        } else {
            "不符合".to_string()
        },
        if campus_ssid { "符合" } else { "不符合" },
        if transport.eq_ignore_ascii_case("wifi") {
            if bssid.is_empty() { "缺失" } else { "已取得" }
        } else {
            "不适用"
        },
        if identity_fresh { "是" } else { "否" },
        if wifi_route_failed { "失败" } else if route_ip == ip { "同源" } else if route_ip.is_empty() { "未知" } else { "独立路由" },
    );
    push_step(make_diagnostic_step(
        "campus_environment",
        "校园网环境",
        campus_started,
        campus_status,
        match campus_status {
            "success" => "符合校园网特征",
            "warning" => "部分校园网特征符合",
            "skipped" => "移动数据，跳过检查",
            _ => "未匹配校园网特征",
        },
        campus_message,
    ));
    emit_network_diagnostic_progress(&app, &run_id, 42, "校园网环境特征检查完成");

    let portal_route_context = portal_route_context_from_network(&network).ok().flatten();
    let dns_probe = async {
        let step = diagnostic_dns::diagnose(&network, campus_ip || campus_ssid, deadline).await;
        emit_step(&app, &run_id, &step);
        step
    };
    let internet_probe = async {
        let started = std::time::Instant::now();
        let direct_supported = matches!(transport.as_str(), "wifi" | "ethernet");
        let mut paths = diagnostic_paths::Paths::pending();
        let mut connection = connectivity::Snapshot::default();
        let mut timed_out = prepared.is_none();
        if prepared.is_some() {
            let observation = connectivity::for_app(&app, &network);
            let result = timeout_at(
                deadline,
                observation.wait_with_updates(|snapshot| {
                    connection = snapshot.clone();
                    if let Some(health) = &snapshot.health {
                        paths.system = health.clone();
                    }
                    if let Some(health) = &snapshot.physical {
                        paths.direct = health.clone();
                    }
                    emit_step(&app, &run_id, &paths.step());
                }),
            )
            .await;
            match result {
                Ok(snapshot) => connection = snapshot,
                Err(_) => timed_out = true,
            }
        }
        paths.finish(
            direct_supported,
            timed_out.then(|| started.elapsed().as_millis()),
        );
        let session_step = connection.require_session.then(|| DiagnosticStep {
            id: "authentication_session".into(),
            label: "认证网卡的校园会话".into(),
            status: if connection.session_online == Some(true) {
                "success"
            } else {
                "warning"
            }
            .into(),
            summary: match connection.session_online {
                Some(true) => "已有校园会话",
                Some(false) => "尚未登录校园网",
                None if timed_out && !connection.session_checked => "检查超时",
                None => "未确认校园会话",
            }
            .into(),
            message: match connection.session_online {
                Some(true) => "所选认证网卡已有校园在线会话",
                Some(false) => "所选认证网卡尚未登录校园网（当前外网可能经过 VPN 或其他网卡）",
                None if timed_out && !connection.session_checked => {
                    "在 2 秒总预算内未收到认证网关的会话信息"
                }
                None => "未能确认所选网卡的校园网登录状态，请检查认证网关",
            }
            .into(),
            duration_ms: connection.session_duration_ms,
            connectivity: None,
        });
        let mut internet_step = paths.step();
        let system_online = connection.online();
        let online = system_online || (!connection.cancelled && paths.direct.online());
        if !paths.system.online() && system_online {
            for result in &connection.internet {
                internet_step.message.push_str(&format!(
                    "\n系统路由补充目标 {}：{}",
                    result.label, result.detail
                ));
            }
            internet_step.status = "success".into();
            internet_step.summary = "补充目标验证通过".into();
        }
        let dual_stack = if connection
            .preferred_health()
            .is_some_and(|health| health.scope == "physical")
        {
            paths.direct.clone()
        } else {
            paths.system
        };
        if let Some(step) = &session_step {
            emit_step(&app, &run_id, step);
        }
        (
            online,
            internet_step,
            dual_stack,
            session_step,
            connection.cancelled && !wifi_route_failed,
            system_online && direct_supported && !paths.direct.online_for(connection.auth_stack),
        )
    };
    let gateway_probe = async {
        let started = std::time::Instant::now();
        let budget = deadline
            .saturating_duration_since(Deadline::now())
            .saturating_sub(std::time::Duration::from_millis(10));
        if wifi_route_failed || portal_route_context.is_none() || budget.is_zero() {
            return (Vec::new(), None, 0);
        }
        let (gateways, ipv6) = timeout_at(
            deadline,
            futures_util::future::join(
                diagnose_login_gateways(
                    compatibility,
                    &ssid,
                    &transport,
                    portal_route_context.as_ref(),
                    budget,
                ),
                async {
                    if lgn_wired_hint {
                        Some(
                            diagnose_lgn_ipv6_rust(
                                compatibility,
                                portal_route_context.as_ref(),
                                budget,
                            )
                            .await,
                        )
                    } else {
                        None
                    }
                },
            ),
        )
        .await
        .unwrap_or_default();
        (gateways, ipv6, started.elapsed().as_millis())
    };
    let (
        dns_step,
        (
            online,
            internet_step,
            mut dual_stack,
            session_step,
            probe_cancelled,
            physical_unconfirmed,
        ),
        (gateway_results, lgn_ipv6_diagnostic, portal_duration_ms),
    ) = run_diagnostic_probes(
        deadline,
        dns_probe,
        internet_probe,
        gateway_probe,
        |percent| {
            emit_network_diagnostic_progress(
                &app,
                &run_id,
                percent,
                "正在并发检查 DNS、互联网和认证网关…",
            )
        },
    )
    .await;
    push_step(dns_step);
    push_step(internet_step);
    let session_unconfirmed = session_step
        .as_ref()
        .is_some_and(|step| step.status != "success");
    if let Some(step) = session_step {
        push_step(step);
    }
    // diagnose_login_gateways preserves the same physical-link priority used
    // by automatic login. Prefer the first verified portal even if a lower
    // priority gateway becomes reachable only after authentication.
    let detection = gateway_results.iter().find(|result| result.portal_detected);
    let login_type = detection
        .filter(|result| result.login_ready)
        .map(|result| result.login_type.clone())
        .unwrap_or(LoginType::Unknown);
    let type1_requires_maximum =
        detection.is_some_and(|result| type1_portal_requires_maximum(result, compatibility));
    let mut gateway_details = gateway_results
        .iter()
        .map(|result| {
            format!(
                "{}：{}",
                result.login_type.display_name(),
                if result.login_ready {
                    "登录接口与响应校验通过"
                } else if result.portal_detected {
                    "已发现认证页，暂无法直接登录"
                } else if result.timed_out {
                    "在本次 2 秒诊断预算内未完成"
                } else {
                    "未探测到"
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    #[cfg(target_os = "macos")]
    let adapter_restart = if lgn_wired_hint {
        let mut configuration_issue = false;
        if let Some(result) = lgn_ipv6_diagnostic.as_ref() {
            if let Ok(configuration) = serde_json::from_value::<macos_network::LgnLinkConfiguration>(
                network
                    .get("lgnLinkConfiguration")
                    .cloned()
                    .unwrap_or_default(),
            ) {
                configuration_issue = configuration.resembles_reported_ipv6_failure();
                push_step(DiagnosticStep {
                    id: "wired_ipv6_configuration".to_string(),
                    label: "有线 IPv6 配置".to_string(),
                    status: if result.is_ok() && !configuration_issue {
                        "success"
                    } else {
                        "warning"
                    }
                    .to_string(),
                    summary: if result.is_ok() && !configuration_issue {
                        "IPv6 配置已验证"
                    } else {
                        "IPv6 配置需核对"
                    }
                    .into(),
                    message: configuration.diagnostic(result.is_ok()),
                    duration_ms: 0,
                    connectivity: None,
                });
            }
        }
        (configuration_issue || lgn_ipv6_diagnostic.as_ref().is_some_and(Result::is_err)).then(
            || AdapterRestartTarget {
                interface_name: interface_name.to_string(),
                ipv4: ip.clone(),
                reason: if configuration_issue {
                    "IPv6 配置与已报告的异常组合一致"
                } else {
                    "lgn6 IPv6 地址发现未通过"
                }
                .to_string(),
            },
        )
    } else {
        None
    };
    #[cfg(not(target_os = "macos"))]
    let adapter_restart = None;
    if let Some(result) = lgn_ipv6_diagnostic {
        gateway_details.push_str("\nlgn IPv6 地址发现：");
        match result {
            Ok(detail) => gateway_details.push_str(&detail),
            Err(error) => gateway_details.push_str(&format!("不可用（{error}）")),
        }
    }
    let (portal_status, portal_summary, portal_message) = if wifi_route_failed {
        (
            "error",
            "绑定网卡失败".to_string(),
            "无法绑定非默认校园 Wi-Fi，已停止认证网关探测以避免误用 VPN/移动数据".to_string(),
        )
    } else if portal_route_context.is_none() {
        (
            "skipped",
            "缺少可用的认证网卡".to_string(),
            "未取得同一物理网卡的接口名和所需地址，未发起认证网关探测。".to_string(),
        )
    } else if login_type != LoginType::Unknown {
        (
            if online { "success" } else { "warning" },
            format!("{} 认证网关可用", login_type.display_name()),
            format!(
                "判定校园网类型为 {}。\n{}",
                login_type.display_name(),
                gateway_details
            ),
        )
    } else if type1_requires_maximum {
        (
            "warning",
            "已发现宿舍网关，需要核对连接方式".to_string(),
            format!("已发现 bjut-sushe 认证网关，但当前 HTTPS 兼容模式不能向其发送账号密码。\n确认网络可信后可临时启用最高兼容（HTTP + IP）。\n{gateway_details}"),
        )
    } else if gateway_results.is_empty() || gateway_results.iter().any(|result| result.timed_out) {
        (
            "warning",
            "网关检查未完成".to_string(),
            format!("部分认证网关在 2 秒总预算内未返回可验证的响应。\n{gateway_details}"),
        )
    } else {
        (
            if online { "skipped" } else { "warning" },
            "未发现可用的校园认证网关".to_string(),
            format!(
                "未找到可访问的校园网认证网关，可能是完全离线或处于非校园网络。\n{gateway_details}"
            ),
        )
    };
    push_step(DiagnosticStep {
        id: "portal".to_string(),
        label: "认证网关".to_string(),
        status: portal_status.to_string(),
        summary: portal_summary,
        message: portal_message,
        duration_ms: portal_duration_ms,
        connectivity: None,
    });
    emit_network_diagnostic_progress(&app, &run_id, 100, "网络链路诊断完成");

    let stale = probe_cancelled
        || generation
            != app
                .state::<Arc<AppState>>()
                .network_change_generation
                .load(Ordering::SeqCst);
    if stale {
        dual_stack.invalidate("网络或认证状态已改变，此报告仅保留原检测明细，请重新诊断");
    }
    let adapter_restart = if stale { None } else { adapter_restart };
    let (overall, summary) = if stale {
        ("changed", "检测期间网络或认证状态已改变，请重新诊断")
    } else if physical_unconfirmed && online {
        if network["captivePortal"].as_bool().unwrap_or(false) {
            (
                "auth_required",
                "当前可通过其他路径上网，但此 Wi-Fi 仍需登录校园网",
            )
        } else {
            (
                "partial",
                "当前可通过其他路径上网，但所选网卡直连测试未通过",
            )
        }
    } else if session_unconfirmed && online {
        ("partial", "外网已通，但所选网卡的校园网登录状态待确认")
    } else if prepared.is_none() {
        ("partial", "未在诊断预算内取得网络接口信息")
    } else if online && (address.is_empty() || transport == "none") {
        ("partial", "外网已通，但认证网卡尚未获取到 IP 地址")
    } else if online && !dual_stack.online() {
        ("partial", "外网已通，IPv4 / IPv6 分项连接测试尚未完成")
    } else if online
        && dual_stack.ipv6.status != "reachable"
        && dual_stack.ipv6.status != "not_configured"
    {
        ("partial", "外网已通，IPv6 连接测试未通过")
    } else if online
        && dual_stack.ipv4.status != "reachable"
        && dual_stack.ipv4.status != "not_configured"
    {
        ("partial", "外网已通，IPv4 连接测试未通过")
    } else if online {
        ("healthy", "网络正常，互联网已连通")
    } else if login_type != LoginType::Unknown {
        ("auth_required", "已连接校园网，需要登录账号")
    } else if type1_requires_maximum {
        (
            "auth_required",
            "已连接宿舍校园网；该网关仅支持 HTTP，请切换至“最高兼容”后再登录",
        )
    } else if address.is_empty() || transport == "none" {
        (
            "no_network",
            "未获取到网络地址，请检查 Wi-Fi、网线或系统权限",
        )
    } else if mobile_data {
        ("offline", "当前为移动数据网络，外网连接测试未通过")
    } else {
        (
            "offline",
            "已取得本地网络，但无法访问互联网或校园网认证网关",
        )
    };
    DiagnosticReport {
        stale,
        created_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        overall: overall.to_string(),
        summary: summary.to_string(),
        ssid,
        ip: address.to_string(),
        steps,
        adapter_restart,
        dual_stack,
    }
}

pub(super) fn mask_identifier(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 4 {
        return "****".to_string();
    }
    format!(
        "{}***{}",
        chars.iter().take(2).collect::<String>(),
        chars
            .iter()
            .rev()
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>()
    )
}

pub(super) fn redact_diagnostic_text(
    mut text: String,
    accounts: &[Account],
    ip: &str,
    bssid: &str,
) -> String {
    for account in accounts {
        if !account.user.is_empty() {
            text = text.replace(&account.user, &mask_identifier(&account.user));
        }
        if !account.pass.is_empty() {
            text = text.replace(&account.pass, "[REDACTED]");
        }
    }
    if !ip.is_empty() {
        text = text.replace(ip, "[LOCAL-IP]");
    }
    if !bssid.is_empty() {
        text = text.replace(bssid, "[BSSID]");
    }
    text
}

#[tauri::command]
pub(super) async fn create_diagnostic_bundle(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<String, String> {
    let mut report = run_network_diagnostics(app.clone(), None).await;
    let config = state.config.read().unwrap().clone();
    let network_state = state.last_network_state.lock().unwrap().clone();
    let ip = network_state
        .get("ip")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let bssid = network_state
        .get("bssid")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let logs: Vec<serde_json::Value> = state.logs.lock().unwrap().iter().rev().take(500).rev()
        .map(|entry| serde_json::json!({
            "time": entry.time,
            "module": entry.module,
            "type": entry.log_type,
            "message": redact_diagnostic_text(entry.message.clone(), &config.accounts, ip, bssid),
        }))
        .collect();
    let public_accounts: Vec<Account> = config
        .accounts
        .iter()
        .cloned()
        .map(|mut account| {
            account.pass.clear();
            account
        })
        .collect();
    let detected_ip = std::mem::take(&mut report.ip);
    let redact = |text: String| {
        let text = redact_diagnostic_text(text, &public_accounts, ip, bssid);
        if detected_ip.is_empty() {
            text
        } else {
            text.replace(&detected_ip, "[LOCAL-IP]")
        }
    };
    report.summary = redact(report.summary);
    report.ip = if detected_ip.is_empty() {
        String::new()
    } else {
        "[LOCAL-IP]".to_string()
    };
    for step in &mut report.steps {
        step.summary = redact(std::mem::take(&mut step.summary));
        step.message = redact(std::mem::take(&mut step.message));
    }
    report.dual_stack.redact_addresses();
    if let Some(target) = &mut report.adapter_restart {
        target.ipv4 = "[LOCAL-IP]".to_string();
    }
    let redacted_report = serde_json::to_value(&report).map_err(|error| error.to_string())?;
    let bundle = serde_json::json!({
        "schemaVersion": 1,
        "appVersion": app.package_info().version.to_string(),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "createdAt": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        "configuration": {
            "accountCount": config.accounts.len(),
            "enabledAccountCount": config.accounts.iter().filter(|account| !account.is_disabled.unwrap_or(false)).count(),
            "networkProfileCount": config.network_profiles.len(),
            "autoLogin": config.auto_login,
            "checkInterval": config.check_interval,
            "checkIntervalBackground": config.check_interval_bg,
            "usageAlerts": config.usage_alerts,
        },
        "diagnostic": redacted_report,
        "networkEvents": state.network_events.lock().unwrap().entries(),
        "logs": logs,
        "privacy": "账号、密码、本地 IP 与 BSSID 已脱敏；诊断包不包含凭据。",
    });
    serde_json::to_string_pretty(&bundle).map_err(|error| error.to_string())
}
