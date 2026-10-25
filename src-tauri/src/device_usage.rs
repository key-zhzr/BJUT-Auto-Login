//! Minute samples of jfself's online-device counters, not NIC byte counters.
use std::{
    collections::HashMap,
    sync::{atomic::Ordering, Arc},
};
pub(crate) static INTERACTIVE: tokio::sync::Notify = tokio::sync::Notify::const_new();
fn credential_stamp(password: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(password.as_bytes()).into()
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountUsage {
    pub account: String,
    pub updated_at: i64,
    pub devices: Vec<crate::billing::BillingOnlineSession>,
    pub error: Option<String>,
    #[serde(default)]
    pub balance: String,
    #[serde(default)]
    pub remaining_flow: String,
    #[serde(default)]
    pub monthly_fee: Option<f64>,
    #[serde(skip)]
    pub(crate) next_attempt: i64,
    #[serde(skip)]
    pub(crate) credential_stamp: [u8; 32],
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct Store(
    pub HashMap<String, AccountUsage>,
    pub crate::usage_history::History,
);

fn path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("device-usage.json"))
}
pub(crate) fn load(app: &tauri::AppHandle) -> Store {
    path(app)
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() < 8 * 1024 * 1024))
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice::<Store>(&bytes).ok())
        .map(|mut store| {
            for item in store.0.values_mut() {
                for device in &mut item.devices {
                    device.session_id.clear();
                }
            }
            store
        })
        .unwrap_or_default()
}
fn save(app: &tauri::AppHandle, store: &Store) {
    use std::io::Write;
    let write = || -> Result<(), Box<dyn std::error::Error>> {
        let Some(path) = path(app) else { return Ok(()) };
        std::fs::create_dir_all(path.parent().unwrap())?;
        let temp = path.with_extension("tmp");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(&serde_json::to_vec(store)?)?;
        file.sync_all()?;
        std::fs::rename(temp, path)?;
        Ok(())
    };
    let _ = write();
}
#[tauri::command]
pub(crate) fn get_usage_history(
    state: tauri::State<'_, Arc<crate::AppState>>,
) -> Vec<crate::usage_history::DailyUsage> {
    let config = state.config.read().unwrap();
    state
        .device_usage
        .lock()
        .unwrap()
        .1
        .rows()
        .into_iter()
        .filter(|row| {
            config
                .accounts
                .iter()
                .any(|account| account.user == row.account)
        })
        .collect()
}

#[tauri::command]
pub(crate) fn get_device_usage(state: tauri::State<'_, Arc<crate::AppState>>) -> Vec<AccountUsage> {
    let config = state.config.read().unwrap();
    let store = state.device_usage.lock().unwrap();
    config
        .accounts
        .iter()
        .filter_map(|a| store.0.get(&a.user).cloned())
        .collect()
}

pub(crate) async fn run(app: tauri::AppHandle, state: Arc<crate::AppState>) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        // Android foreground policy also applies to silent billing reads.
        if cfg!(target_os = "android") && state.is_in_background.load(Ordering::SeqCst) {
            continue;
        }
        let config = state.config.read().unwrap().clone();
        if !config.traffic_monitor_enabled {
            continue;
        }
        let now = chrono::Utc::now().timestamp();
        let account = {
            let mut store = state.device_usage.lock().unwrap();
            store
                .0
                .retain(|key, _| config.accounts.iter().any(|a| a.user == *key));
            store.1.retain_accounts(&config.accounts);
            for account in &config.accounts {
                if let Some(item) = store.0.get_mut(&account.user) {
                    let stamp = credential_stamp(&account.pass);
                    if item.credential_stamp != stamp {
                        item.next_attempt = 0;
                        item.credential_stamp = stamp;
                    }
                }
            }
            config
                .accounts
                .iter()
                .filter(|a| !a.pass.is_empty())
                .filter(|a| store.0.get(&a.user).is_none_or(|v| v.next_attempt <= now))
                .min_by_key(|a| store.0.get(&a.user).map_or(0, |v| v.next_attempt))
                .cloned()
        };
        let Some(account) = account else { continue };
        // Interactive operations always take priority; do not build a queue.
        let interrupted = INTERACTIVE.notified();
        tokio::pin!(interrupted);
        interrupted.as_mut().enable();
        let Ok(_guard) = state.billing_fetch_lock.try_lock() else {
            continue;
        };
        let mut access =
            crate::billing::BillingAccess::saved(account.user.clone(), account.pass.clone());
        if let Ok(vpn) = state.billing_webvpn.lock().await.session() {
            access = access.via_webvpn(vpn);
        }
        let compatibility = crate::effective_vpn_compatibility(&config);
        let generation = state.network_change_generation.load(Ordering::SeqCst);
        let query = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            Box::pin(crate::billing::query_device_usage(
                &access,
                compatibility,
                &state.billing_sessions,
            )),
        );
        let result = match futures_util::future::select(interrupted, Box::pin(query)).await {
            futures_util::future::Either::Left(_) => continue,
            futures_util::future::Either::Right((result, _)) => result,
        };
        // Do not publish a response for credentials removed/changed in flight.
        if !state
            .config
            .read()
            .unwrap()
            .accounts
            .iter()
            .any(|a| a.user == account.user && a.pass == account.pass)
        {
            continue;
        }
        let mut store = state.device_usage.lock().unwrap();
        let item = store
            .0
            .entry(account.user.clone())
            .or_insert_with(|| AccountUsage {
                account: account.user.clone(),
                ..Default::default()
            });
        item.credential_stamp = credential_stamp(&account.pass);
        if generation != state.network_change_generation.load(Ordering::SeqCst) {
            item.error = Some("网络已变化，等待重新查询".into());
            item.next_attempt = now + 60;
            continue;
        }
        match result {
            Ok(Ok((overview, devices))) => {
                item.devices = devices;
                for device in &mut item.devices {
                    device.session_id.clear();
                }
                item.balance = overview.balance;
                item.remaining_flow = overview.remaining_flow;
                item.monthly_fee = crate::month_end::monthly_fee(
                    overview.package.as_deref().unwrap_or_default(),
                    overview.package_detail.as_deref().unwrap_or_default(),
                )
                .map(|fee| fee as f64 / 10000.);
                item.updated_at = chrono::Utc::now().timestamp();
                item.error = None;
                item.next_attempt = now + 60;
            }
            Ok(Err(error)) => {
                item.error = Some(error.user_message());
                item.next_attempt = if matches!(
                    error,
                    crate::billing::BillingError::CaptchaRequired
                        | crate::billing::BillingError::AuthenticationRejected
                ) {
                    i64::MAX
                } else {
                    now + 300
                };
            }
            Err(_) => {
                item.error = Some("设备流量查询超时".into());
                item.next_attempt = now + 300;
            }
        }
        let data = item.clone();
        if data.error.is_none() {
            store
                .1
                .observe(&data.account, data.updated_at, &data.devices);
        }
        save(&app, &store);
        drop(store);
        let _ = app.emit("device-usage", data);
    }
}
use tauri::Emitter;
