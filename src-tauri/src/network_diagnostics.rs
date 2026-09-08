//! Compose read-only diagnostics and export a redacted support bundle.
use super::*;

#[tauri::command]
pub(super) async fn run_network_diagnostics(app: tauri::AppHandle) -> DiagnosticReport {
    let generation = app
        .state::<Arc<AppState>>()
        .network_change_generation
        .load(Ordering::SeqCst);
    emit_network_diagnostic_progress(&app, 2, "正在读取当前网络接口…");
    let compatibility = app
        .try_state::<Arc<AppState>>()
        .map(|state| {
            let config = state.config.read().unwrap();
            effective_vpn_compatibility(&config)
        })
        .unwrap_or(VpnCompatibility::High);
    let mut steps = Vec::new();
    let identity_started = std::time::Instant::now();
    let network = get_network_info(app.clone(), Some(true));
    #[cfg(target_os = "android")]
    let wifi_route_guard = AndroidWifiRouteGuard::bind_if_required(&network);
    #[cfg(target_os = "android")]
    let wifi_route_failed = wifi_route_guard.failed();
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
    let mut ip = network
        .get("ip")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if ip.is_empty() {
        ip = get_local_ip(app.clone());
    }
    let identity_status = if ip.is_empty() {
        "error"
    } else if !wifi_identity_error.is_empty() {
        "warning"
    } else if mobile_data || !ssid.is_empty() || transport.eq_ignore_ascii_case("ethernet") {
        "success"
    } else {
        "warning"
    };
    let identity_message = if ip.is_empty() {
        "未检测到可用网络接口或 IPv4 地址".to_string()
    } else if mobile_data {
        format!("当前通过移动数据上网，本地 IP {ip}")
    } else if wifi_identity_error == "locationPermissionDenied" {
        format!(
            "已取得接口 {} 的本地 IP {ip}，但 Windows 未授予精确位置权限，无法读取 SSID/BSSID；请在系统“隐私和安全性 → 位置”中允许桌面应用访问位置",
            if interface_name.is_empty() {
                "未知"
            } else {
                interface_name
            }
        )
    } else if wifi_identity_error == "locationPermissionOrServiceUnavailable" {
        format!(
            "已取得接口 {} 的本地 IP {ip}，但 Android 未返回 SSID/BSSID；请确认附近 Wi-Fi、前台位置权限和系统位置服务均已开启",
            if interface_name.is_empty() {
                "未知"
            } else {
                interface_name
            }
        )
    } else if transport.eq_ignore_ascii_case("ethernet") {
        format!(
            "已连接有线接口 {}，本地 IP {ip}（身份来源：{identity_source}）",
            if interface_name.is_empty() {
                "未知"
            } else {
                interface_name
            }
        )
    } else if ssid.is_empty() || ssid.eq_ignore_ascii_case("<unknown ssid>") {
        format!("已取得本地 IP {ip}，但未取得无线网络名称（可能为有线网络或权限不足）")
    } else {
        format!(
            "已连接 {ssid}，接口 {}，本地 IP {ip}（身份来源：{identity_source}）",
            if interface_name.is_empty() {
                "未知"
            } else {
                interface_name
            }
        )
    };
    steps.push(make_diagnostic_step(
        "network_identity",
        "网络接口",
        identity_started,
        identity_status,
        identity_message,
    ));
    emit_network_diagnostic_progress(&app, 16, "网络接口信息读取完成");

    let route_started = std::time::Instant::now();
    emit_network_diagnostic_progress(&app, 20, "正在核对校园认证目标路由…");
    let route_uses_physical_ipv4 = usable_physical_ipv4(route_ip).is_some();
    steps.push(make_diagnostic_step(
        "campus_route",
        "校园目标路由",
        route_started,
        if wifi_route_failed {
            "error"
        } else if route_ip.is_empty() || !route_uses_physical_ipv4 || ip.is_empty() {
            "warning"
        } else {
            "success"
        },
        if wifi_route_failed {
            "检测到非默认的待认证 Wi-Fi，但 Android 无法将请求绑定到该网络".to_string()
        } else if route_ip.is_empty() {
            "无法取得访问校园认证目标时系统选择的源 IPv4".to_string()
        } else if !route_uses_physical_ipv4 {
            format!(
                "校园认证目标当前走 {route_ip}，该地址像 VPN/TUN Fake-IP；不会将它当作物理网卡地址"
            )
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
    emit_network_diagnostic_progress(&app, 30, "校园认证目标路由核对完成");

    let campus_started = std::time::Instant::now();
    emit_network_diagnostic_progress(&app, 34, "正在检查校园网环境特征…");
    let campus_ip = is_campus_local_ip(&ip);
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
        "传输类型：{}\n本地网段：{}\nlgn 有线特征：{}\nSSID 特征：{}\nWi-Fi BSSID：{}\n身份同接口且新鲜：{}\n校园目标路由绑定：{}",
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
    steps.push(make_diagnostic_step(
        "campus_environment",
        "校园网环境",
        campus_started,
        campus_status,
        campus_message,
    ));
    emit_network_diagnostic_progress(&app, 42, "校园网环境特征检查完成");

    let portal_route_context = portal_route_context_from_network(&network).ok().flatten();
    let dns_probe = async {
        let started = std::time::Instant::now();
        let public_dns = async {
            match tokio::time::timeout(
                NETWORK_PROBE_TIMEOUT,
                tokio::net::lookup_host(("www.baidu.com", 443)),
            )
            .await
            {
                Ok(Ok(mut addresses)) => {
                    if addresses.next().is_some() {
                        (true, "系统 DNS 解析正常".to_string())
                    } else {
                        (false, "系统 DNS 未返回可用地址".to_string())
                    }
                }
                Ok(Err(error)) => (false, format!("系统 DNS 解析失败：{error}")),
                Err(_) => (false, "系统 DNS 解析超过 3 秒".to_string()),
            }
        };
        let campus_dns = async {
            if compatibility != VpnCompatibility::Low || wifi_route_failed {
                return None;
            }
            let route = portal_route_context.as_ref()?;
            let host = if lgn_wired_hint { LGN_HOST } else { WLGN_HOST };
            let servers = campus_dns_servers(host, Some(route.physical_ipv4())).join(" / ");
            Some(
                match query_campus_dns_ipv4(host, Some(route.physical_ipv4())).await {
                    Ok(_) => (true, format!("校园 DNS（{servers}）：{host} 解析成功")),
                    Err(error) => (false, format!("校园 DNS（{servers}）：{error}")),
                },
            )
        };
        let ((mut ok, mut message), campus) =
            futures_util::future::join(public_dns, campus_dns).await;
        if let Some((campus_ok, detail)) = campus {
            ok &= campus_ok;
            message.push('\n');
            message.push_str(&detail);
        }
        make_diagnostic_step(
            "dns",
            "DNS 解析",
            started,
            if ok { "success" } else { "warning" },
            message,
        )
    };
    let internet_probe = async {
        let connection = if wifi_route_failed {
            connectivity::Snapshot {
                cancelled: true,
                complete: true,
                ..Default::default()
            }
        } else {
            connectivity::for_app(&app, &network).wait(true).await
        };
        let session_step = connection.require_session.then(|| DiagnosticStep {
            id: "authentication_session".into(),
            label: "认证网卡的校园会话".into(),
            status: if connection.session_online == Some(true) {
                "success"
            } else {
                "warning"
            }
            .into(),
            message: match connection.session_online {
                Some(true) => "所选认证网卡已有校园在线会话",
                Some(false) => {
                    "所选认证网卡尚无校园在线会话；其他网卡或 VPN 的互联网可用不代表此网卡已认证"
                }
                None => "暂未确认所选网卡的校园会话；请核对网关与认证状态",
            }
            .into(),
            duration_ms: connection.session_duration_ms,
        });
        let results = connection.internet;
        let duration_ms = connection.internet_duration_ms;
        let dual_stack = if connection.cancelled {
            dual_stack::unavailable("检测期间网络或认证状态改变，请重新诊断")
        } else {
            connection
                .health
                .unwrap_or_else(|| dual_stack::unavailable("独立探测尚未完成"))
        };
        let online = !connection.cancelled && results.iter().any(|result| result.success);
        let message = if results.is_empty() {
            "未执行互联网目标探测".to_string()
        } else {
            results
                .iter()
                .map(|result| {
                    format!(
                        "{}：{}（{}）",
                        result.label,
                        if result.success { "成功" } else { "失败" },
                        result.detail
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        (
            online || dual_stack.online(),
            DiagnosticStep {
                id: "internet".to_string(),
                label: "互联网连通性".to_string(),
                status: if online { "success" } else { "warning" }.to_string(),
                message,
                duration_ms,
            },
            dual_stack,
            session_step,
            connection.cancelled && !wifi_route_failed,
        )
    };
    let gateway_probe = async {
        let started = std::time::Instant::now();
        let result = if wifi_route_failed || portal_route_context.is_none() {
            (Vec::new(), None)
        } else {
            futures_util::future::join(
                diagnose_login_gateways(
                    compatibility,
                    &ssid,
                    network_transport(&network),
                    portal_route_context.as_ref(),
                ),
                async {
                    if lgn_wired_hint {
                        Some(
                            diagnose_lgn_ipv6_rust(compatibility, portal_route_context.as_ref())
                                .await,
                        )
                    } else {
                        None
                    }
                },
            )
            .await
        };
        (result, started.elapsed().as_millis())
    };
    let (
        dns_step,
        (online, internet_step, mut dual_stack, session_step, probe_cancelled),
        ((gateway_results, lgn_ipv6_diagnostic), portal_duration_ms),
    ) = run_diagnostic_probes(dns_probe, internet_probe, gateway_probe, |percent| {
        emit_network_diagnostic_progress(&app, percent, "正在并发检查 DNS、互联网和认证网关…")
    })
    .await;
    steps.push(dns_step);
    steps.push(internet_step);
    let session_unconfirmed = session_step
        .as_ref()
        .is_some_and(|step| step.status != "success");
    if let Some(step) = session_step {
        steps.push(step);
    }
    for (id, label, family) in [
        ("ipv4_internet", "IPv4 互联网", &dual_stack.ipv4),
        ("ipv6_internet", "IPv6 互联网", &dual_stack.ipv6),
    ] {
        steps.push(DiagnosticStep {
            id: id.to_string(),
            label: label.to_string(),
            status: if family.status == "reachable" {
                "success"
            } else if family.status == "not_configured" {
                "skipped"
            } else {
                "warning"
            }
            .to_string(),
            message: family.detail.clone(),
            duration_ms: family.duration_ms,
        });
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
                    "门户已发现，但登录条件未完全就绪"
                } else if result.timed_out {
                    "在 3 秒诊断预算内无响应"
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
                steps.push(DiagnosticStep {
                    id: "wired_ipv6_configuration".to_string(),
                    label: "有线 IPv6 配置".to_string(),
                    status: if result.is_ok() && !configuration_issue {
                        "success"
                    } else {
                        "warning"
                    }
                    .to_string(),
                    message: configuration.diagnostic(result.is_ok()),
                    duration_ms: 0,
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
    let (portal_status, portal_message) = if wifi_route_failed {
        (
            "error",
            "无法绑定非默认校园 Wi-Fi，已停止认证网关探测以避免误用 VPN/移动数据".to_string(),
        )
    } else if portal_route_context.is_none() {
        (
            "error",
            "未取得同一物理接口的名称与 IPv4，已阻止不绑定接口的认证网关探测。\n请检查 Windows 网卡驱动与接口权限。".to_string(),
        )
    } else if login_type != LoginType::Unknown {
        (
            if online { "success" } else { "warning" },
            format!(
                "判定校园网类型为 {}。\n{}",
                login_type.display_name(),
                gateway_details
            ),
        )
    } else if type1_requires_maximum {
        (
            "warning",
            format!("已发现 bjut-sushe 认证网关，但当前 HTTPS 兼容模式不能向其发送账号密码。\n确认网络可信后可临时启用最高兼容（HTTP + IP）。\n{gateway_details}"),
        )
    } else if online {
        (
            "success",
            format!("互联网已联通，校园认证网关仍已逐项探测。\n{gateway_details}"),
        )
    } else {
        (
            "error",
            format!(
                "未找到可访问的校园网认证网关，可能是完全离线或处于非校园网络。\n{gateway_details}"
            ),
        )
    };
    steps.push(DiagnosticStep {
        id: "portal".to_string(),
        label: "认证网关".to_string(),
        status: portal_status.to_string(),
        message: portal_message,
        duration_ms: portal_duration_ms,
    });
    emit_network_diagnostic_progress(&app, 100, "网络链路诊断完成");

    let latest = get_network_info(app.clone(), Some(false));
    let stale = probe_cancelled
        || generation
            != app
                .state::<Arc<AppState>>()
                .network_change_generation
                .load(Ordering::SeqCst)
        || latest["interfaceName"] != network["interfaceName"]
        || latest["ip"] != network["ip"];
    if stale {
        dual_stack.invalidate("网络或认证状态已改变，此报告仅保留原检测明细，请重新诊断");
    }
    let adapter_restart = if stale { None } else { adapter_restart };
    let (overall, summary) = if stale {
        ("changed", "检测期间网络或认证状态已改变，请重新诊断")
    } else if session_unconfirmed && online {
        ("partial", "系统互联网可达，所选认证网卡的校园会话尚未确认")
    } else if online && (ip.is_empty() || transport == "none") {
        ("partial", "系统互联网可达，但认证网卡尚未取得可用地址")
    } else if online && !dual_stack.online() {
        (
            "partial",
            "系统互联网可达，但暂未完成 IPv4/IPv6 独立探测，请查看各项结果",
        )
    } else if online
        && dual_stack.ipv6.status != "reachable"
        && dual_stack.ipv6.status != "not_configured"
    {
        (
            "partial",
            "互联网可达，IPv6 独立探测暂未通过（不等于 IPv6 不可用）",
        )
    } else if online
        && dual_stack.ipv4.status != "reachable"
        && dual_stack.ipv4.status != "not_configured"
    {
        (
            "partial",
            "互联网可达，IPv4 独立探测暂未通过（不等于 IPv4 不可用）",
        )
    } else if online {
        ("healthy", "网络工作正常，互联网已连通")
    } else if login_type != LoginType::Unknown {
        ("auth_required", "已连接校园网，但需要完成账号认证")
    } else if type1_requires_maximum {
        (
            "auth_required",
            "已连接宿舍校园网；认证网关仅支持 HTTP，等待用户确认最高兼容模式",
        )
    } else if ip.is_empty() || transport == "none" {
        (
            "no_network",
            "未取得网络地址，请检查 Wi-Fi、有线连接或系统权限",
        )
    } else if mobile_data {
        (
            "offline",
            "移动数据存在，但 App 自身的独立目标未验证互联网连通性",
        )
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
        ip,
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
    let mut report = run_network_diagnostics(app.clone()).await;
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
    report.summary = redact_diagnostic_text(report.summary, &public_accounts, ip, bssid);
    report.ip = if report.ip.is_empty() {
        String::new()
    } else {
        "[LOCAL-IP]".to_string()
    };
    for step in &mut report.steps {
        step.message = redact_diagnostic_text(
            std::mem::take(&mut step.message),
            &public_accounts,
            ip,
            bssid,
        );
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
