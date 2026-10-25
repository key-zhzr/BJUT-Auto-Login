//! Campus recharge/session commands and their bounded state transitions.
use super::*;

pub(crate) fn campus_service_target(
    state: &AppState,
    account_user: Option<&str>,
) -> Result<Account, String> {
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

pub(crate) fn campus_service_session_seed(
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

pub(crate) fn persist_campus_service_session(
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

pub(crate) fn persist_recharge_transaction(
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

pub(crate) fn save_recharge_snapshot_verified(
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

pub(crate) fn transition_recharge_transaction(
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

pub(crate) fn transition_recharge_transaction_with_parent(
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
pub(crate) fn get_recoverable_recharges(
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
pub(crate) fn finish_recharge_recovery(
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

pub(crate) fn campus_recharge_open_at_hour(hour: u64) -> bool {
    (6..23).contains(&hour)
}

pub(crate) fn ensure_campus_recharge_open() -> Result<(), String> {
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
pub(crate) async fn prepare_network_recharge(
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
pub(crate) async fn confirm_network_recharge(
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
pub(crate) async fn get_network_recharge_balances(
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
pub(crate) async fn cancel_network_recharge(
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
pub(crate) async fn prepare_alipay_card_recharge(
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
pub(crate) async fn confirm_alipay_card_recharge(
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
pub(crate) async fn cancel_alipay_card_recharge(
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
pub(crate) async fn prepare_wechat_card_recharge(
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
pub(crate) async fn confirm_wechat_card_recharge(
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
pub(crate) async fn check_wechat_card_recharge(
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
pub(crate) async fn cancel_wechat_card_recharge(
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
