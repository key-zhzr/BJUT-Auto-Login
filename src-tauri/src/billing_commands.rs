//! Billing IPC orchestration. Protocol parsing remains in billing/.
use super::*;

#[tauri::command]
pub(crate) async fn get_remaining_flow(
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
pub(crate) async fn get_billing_center(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    account_user: Option<String>,
    current_session: Option<bool>,
    webvpn: Option<bool>,
) -> Result<billing::BillingCenterData, String> {
    emit_billing_center_progress(&app, &state, "准备读取计费中心完整数据", 2);
    device_usage::INTERACTIVE.notify_waiters();
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
        webvpn.unwrap_or(false),
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
        match access
            .with_timeout(
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
    if result.is_ok() {
        if let Some(item) = state
            .device_usage
            .lock()
            .unwrap()
            .0
            .get_mut(&access.account)
        {
            item.next_attempt = 0;
        }
    }
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
pub(crate) async fn query_billing_records(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    query: billing::BillingRecordQuery,
    account_user: Option<String>,
    current_session: Option<bool>,
    webvpn: Option<bool>,
) -> Result<billing::BillingRecordResult, String> {
    let kind = query.kind.clone();
    let page = query.page;
    let all = query.all;
    device_usage::INTERACTIVE.notify_waiters();
    let _fetch_guard = state.billing_fetch_lock.lock().await;
    let (access, compatibility) = resolve_billing_access(
        &app,
        &state,
        account_user.as_deref(),
        current_session.unwrap_or(false),
        false,
        webvpn.unwrap_or(false),
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
pub(crate) async fn perform_billing_action(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    request: billing::BillingActionRequest,
    account_user: Option<String>,
    current_session: Option<bool>,
    webvpn: Option<bool>,
) -> Result<billing::BillingActionResult, String> {
    device_usage::INTERACTIVE.notify_waiters();
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
            webvpn.unwrap_or(false),
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
