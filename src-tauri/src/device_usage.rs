//! Minute samples of jfself's online-device counters, not NIC byte counters.
use std::{
    collections::HashMap,
    sync::{atomic::Ordering, Arc},
};
pub(crate) static INTERACTIVE: tokio::sync::Notify = tokio::sync::Notify::const_new();
pub(crate) fn account_changed(state: &crate::AppState, user: &str) {
    if !state.config.read().unwrap().traffic_monitor_enabled {
        return;
    }
    INTERACTIVE.notify_waiters();
    if let Some(item) = state.device_usage.lock().unwrap().0.get_mut(user) {
        item.next_attempt = 0;
        item.error = None;
    }
}
fn credential_stamp(password: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(password.as_bytes()).into()
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountUsage {
    pub account: String,
    pub updated_at: i64,
    pub devices: Vec<UsageDevice>,
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
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageDevice {
    #[serde(flatten)]
    pub device: crate::billing::BillingOnlineSession,
    #[serde(default)]
    pub interface_id: String,
    #[serde(default)]
    pub interface_name: String,
    #[serde(default)]
    pub local: bool,
}
fn classify(
    device: crate::billing::BillingOnlineSession,
    adapters: &[crate::network_inventory::NetworkAdapter],
) -> UsageDevice {
    let matches = |address: &str, known: &str| {
        address
            .parse::<std::net::IpAddr>()
            .ok()
            .zip(known.parse::<std::net::IpAddr>().ok())
            .is_some_and(|(a, b)| a == b)
    };
    let adapter = adapters.iter().find(|adapter| {
        adapter.connected
            && matches!(
                adapter.transport.as_str(),
                "wifi" | "ethernet" | "cellular" | "mobile"
            )
            && adapter
                .ipv4
                .iter()
                .chain(&adapter.ipv6)
                .any(|ip| matches(&device.ip, ip) || matches(&device.ipv6, ip))
    });
    let identity = if device.mac.is_empty() {
        device.ip.clone()
    } else {
        device.mac.replace([':', '-'], "").to_ascii_lowercase()
    };
    let interface_id = adapter.map_or_else(|| format!("device:{identity}"), |a| a.id.clone());
    let interface_name = adapter.map_or_else(
        || {
            format!(
                "其他设备 {}",
                if device.mac.is_empty() {
                    &device.ip
                } else {
                    &device.mac
                }
            )
        },
        |a| {
            if a.name == a.interface_name {
                a.name.clone()
            } else {
                format!("{} ({})", a.name, a.interface_name)
            }
        },
    );
    UsageDevice {
        device,
        interface_id,
        interface_name,
        local: adapter.is_some(),
    }
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageReport {
    enabled: bool,
    scope: String,
    accounts: Vec<AccountUsage>,
    history: Vec<crate::usage_history::DailyUsage>,
    series: Vec<crate::usage_history::UsagePoint>,
    retention: crate::usage_history::Retention,
}
#[tauri::command]
pub(crate) fn get_usage_report(
    state: tauri::State<'_, Arc<crate::AppState>>,
    query: Option<crate::usage_history::Query>,
) -> UsageReport {
    let config = state.config.read().unwrap();
    if !config.traffic_monitor_enabled {
        return UsageReport {
            enabled: false,
            scope: config.traffic_monitor_scope.clone(),
            accounts: vec![],
            history: vec![],
            series: vec![],
            retention: config.traffic_retention.clone(),
        };
    }
    let store = state.device_usage.lock().unwrap();
    let all = config.traffic_monitor_scope == "all";
    let accounts = config
        .accounts
        .iter()
        .filter_map(|a| store.0.get(&a.user))
        .cloned()
        .map(|mut a| {
            a.devices.retain(|d| all || d.local);
            a
        })
        .collect();
    let saved = |account: &str| config.accounts.iter().any(|a| a.user == account);
    UsageReport {
        enabled: config.traffic_monitor_enabled,
        scope: config.traffic_monitor_scope.clone(),
        accounts,
        history: store
            .1
            .rows()
            .into_iter()
            .filter(|r| saved(&r.account) && (all || r.local))
            .collect(),
        series: store
            .1
            .points()
            .into_iter()
            .filter(|r| {
                saved(&r.account)
                    && (all || r.local)
                    && query.as_ref().is_none_or(|query| query.accepts(r))
            })
            .collect(),
        retention: config.traffic_retention.clone(),
    }
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
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() < 256 * 1024 * 1024))
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice::<Store>(&bytes).ok())
        .map(|mut store| {
            for item in store.0.values_mut() {
                for device in &mut item.devices {
                    device.device.session_id.clear();
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
                && (config.traffic_monitor_scope == "all" || row.local)
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
        .map(|mut a| {
            a.devices
                .retain(|d| config.traffic_monitor_scope == "all" || d.local);
            a
        })
        .collect()
}

// Successful accounts share one minute deadline. Error backoffs stay per account.
fn due(item: Option<&AccountUsage>, now: i64, round: bool) -> bool {
    item.is_none_or(|item| item.next_attempt <= now || (round && item.error.is_none()))
}

type SampleResult = Result<
    Result<
        (
            crate::billing::BillingSnapshot,
            Vec<crate::billing::BillingOnlineSession>,
            i64,
        ),
        crate::billing::BillingError,
    >,
    tokio::time::error::Elapsed,
>;

pub(crate) async fn sample_barrier(at: Option<tokio::time::Instant>) {
    if let Some(at) = at {
        tokio::time::sleep_until(at).await;
    }
}

async fn sample_account(
    state: &crate::AppState,
    config: &crate::AppConfig,
    account: crate::Account,
    sample_at: Option<tokio::time::Instant>,
) -> Option<(crate::Account, i64, SampleResult)> {
    let interrupted = INTERACTIVE.notified();
    tokio::pin!(interrupted);
    interrupted.as_mut().enable();
    // Only interactive requests own this gate. Independent read sessions can run
    // together; a user operation cancels them before acquiring the same gate.
    let gate = state.billing_fetch_lock.try_lock().ok()?;
    drop(gate);
    let mut access =
        crate::billing::BillingAccess::saved(account.user.clone(), account.pass.clone());
    if let Ok(vpn) = state.billing_webvpn.lock().await.session() {
        access = access.via_webvpn(vpn);
    }
    let query = tokio::time::timeout(
        std::time::Duration::from_secs(35),
        Box::pin(crate::billing::query_device_usage(
            &access,
            crate::effective_vpn_compatibility(config),
            &state.billing_sessions,
            sample_at,
        )),
    );
    let foreground_query = async {
        if cfg!(target_os = "android") {
            crate::billing_runtime::run_read_while_foreground(&state.is_in_background, async {
                Ok(query.await)
            })
            .await
            .ok()
        } else {
            Some(query.await)
        }
    };
    let result = match futures_util::future::select(interrupted, Box::pin(foreground_query)).await {
        futures_util::future::Either::Left(_) | futures_util::future::Either::Right((None, _)) => {
            None
        }
        futures_util::future::Either::Right((Some(result), _)) => {
            Some((account, chrono::Utc::now().timestamp(), result))
        }
    };
    result
}

pub(crate) async fn run(app: tauri::AppHandle, state: Arc<crate::AppState>) {
    use futures_util::{stream::FuturesUnordered, StreamExt};
    let mut previous_scope = String::new();
    let mut paused = true;
    let mut next_round = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        if cfg!(target_os = "android") && state.is_in_background.load(Ordering::SeqCst) {
            continue;
        }
        let config = state.config.read().unwrap().clone();
        if !config.traffic_monitor_enabled
            || previous_scope != config.traffic_monitor_scope
            || paused
        {
            state.device_usage.lock().unwrap().1.pause();
            previous_scope = config.traffic_monitor_scope.clone();
            paused = !config.traffic_monitor_enabled;
            next_round = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
            if paused {
                continue;
            }
        }
        let now = chrono::Utc::now().timestamp();
        let round = tokio::time::Instant::now() + std::time::Duration::from_secs(15) >= next_round;
        let sample_at = round.then_some(next_round);
        if round {
            while next_round <= tokio::time::Instant::now() + std::time::Duration::from_secs(15) {
                next_round += std::time::Duration::from_secs(60);
            }
        }
        let deadline = now
            + next_round
                .saturating_duration_since(tokio::time::Instant::now())
                .as_secs() as i64;
        let accounts = {
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
                .filter(|a| !a.pass.is_empty() && due(store.0.get(&a.user), now, round))
                .cloned()
                .collect::<Vec<_>>()
        };
        if accounts.is_empty() {
            continue;
        }
        let generation = state.network_change_generation.load(Ordering::SeqCst);
        let adapters = tokio::task::spawn_blocking(crate::network_inventory::adapters)
            .await
            .unwrap_or_default();
        let mut queries = accounts
            .into_iter()
            .map(|account| sample_account(&state, &config, account, sample_at))
            .collect::<FuturesUnordered<_>>();
        let mut changed = false;
        let mut completed = Vec::new();
        while let Some(result) = queries.next().await {
            let Some((account, at, result)) = result else {
                continue;
            };
            let at = match &result {
                Ok(Ok((_, _, observed_at))) => *observed_at,
                _ => at,
            };
            {
                let current = state.config.read().unwrap();
                if !current.traffic_monitor_enabled
                    || current.traffic_monitor_scope != config.traffic_monitor_scope
                    || !current
                        .accounts
                        .iter()
                        .any(|a| a.user == account.user && a.pass == account.pass)
                {
                    continue;
                }
            }
            let data = {
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
                    item.next_attempt = at + 5;
                } else {
                    match result {
                        Ok(Ok((overview, devices, _))) => {
                            item.devices = devices
                                .into_iter()
                                .map(|device| classify(device, &adapters))
                                .filter(|device| {
                                    config.traffic_monitor_scope == "all" || device.local
                                })
                                .collect();
                            for device in &mut item.devices {
                                device.device.session_id.clear();
                            }
                            item.balance = overview.balance;
                            item.remaining_flow = overview.remaining_flow;
                            item.monthly_fee = crate::month_end::monthly_fee(
                                overview.package.as_deref().unwrap_or_default(),
                                overview.package_detail.as_deref().unwrap_or_default(),
                            )
                            .map(|fee| fee as f64 / 10000.);
                            item.updated_at = at;
                            item.error = None;
                            item.next_attempt = deadline;
                            while item.next_attempt <= at {
                                item.next_attempt += 60;
                            }
                            completed.push(at);
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
                                at + 300
                            };
                        }
                        Err(_) => {
                            item.error = Some("设备流量查询超时".into());
                            item.next_attempt = at + 300;
                        }
                    }
                }
                let data = item.clone();
                if data.error.is_none() {
                    store.1.observe_with_retention(
                        &data.account,
                        data.updated_at,
                        &data.devices,
                        &config.traffic_retention,
                    );
                }
                changed = true;
                data
            };
            let _ = app.emit("device-usage", data);
        }
        if changed {
            save(&app, &state.device_usage.lock().unwrap());
        }
        if let (Some(first), Some(last)) = (completed.iter().min(), completed.iter().max()) {
            crate::rust_log(
                &app,
                &state,
                "流量统计",
                &format!(
                    "本轮完成 {} 个账号，更新时间差 {} 秒",
                    completed.len(),
                    last - first
                ),
                if last - first > 5 { "warning" } else { "debug" },
            );
        }
    }
}

use tauri::Emitter;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prepared_accounts_read_counters_at_the_same_boundary() {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async {
                use futures_util::future::join_all;
                let boundary = tokio::time::Instant::now() + std::time::Duration::from_millis(120);
                let times = join_all([10, 40, 75].into_iter().map(|preparation| async move {
                    tokio::time::sleep(std::time::Duration::from_millis(preparation)).await;
                    sample_barrier(Some(boundary)).await;
                    tokio::time::Instant::now()
                }))
                .await;
                assert!(times.iter().all(|at| *at >= boundary));
                assert!(
                    times
                        .iter()
                        .max()
                        .unwrap()
                        .duration_since(*times.iter().min().unwrap())
                        < std::time::Duration::from_millis(40)
                );
            });
    }
    #[test]
    fn cohort_realigns_successes_without_overriding_error_backoff() {
        let mut item = AccountUsage {
            next_attempt: 1040,
            ..Default::default()
        };
        assert!(!due(Some(&item), 1000, false));
        assert!(due(Some(&item), 1000, true));
        item.error = Some("查询失败".into());
        assert!(!due(Some(&item), 1000, true));
        assert!(due(Some(&item), 1040, false));
        item.next_attempt = 0;
        assert!(due(Some(&item), 1000, false));
    }
    #[test]
    fn local_identity_uses_physical_addresses_and_normalizes_ipv6() {
        let adapters = vec![
            crate::network_inventory::NetworkAdapter {
                id: "en7".into(),
                name: "USB LAN".into(),
                interface_name: "en7".into(),
                transport: "ethernet".into(),
                ipv4: vec!["172.26.1.10".into()],
                ipv6: vec!["2001:da8:216::a".into()],
                connected: true,
                ..Default::default()
            },
            crate::network_inventory::NetworkAdapter {
                id: "tun0".into(),
                transport: "vpn".into(),
                ipv4: vec!["198.18.0.1".into()],
                connected: true,
                ..Default::default()
            },
        ];
        let device = crate::billing::BillingOnlineSession {
            login_at: "one".into(),
            ip: "2001:da8:216:0:0:0:0:a".into(),
            ipv6: String::new(),
            mac: String::new(),
            duration_minutes: "1".into(),
            used_flow_mb: "10".into(),
            session_id: String::new(),
        };
        let local = classify(device.clone(), &adapters);
        assert!(local.local);
        assert_eq!(local.interface_id, "en7");
        let mut other = device;
        other.ip = "198.18.0.1".into();
        assert!(!classify(other, &adapters).local);
    }
}
