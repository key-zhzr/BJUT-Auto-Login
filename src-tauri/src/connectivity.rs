//! Share one read-only probe run across diagnostics, polling and login verification.
//! Results are scoped to the network and invalidated before authentication changes.
use crate::{dual_stack, internet_probe, AppState};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};
use tokio::sync::watch;

#[derive(Clone, Default)]
pub(crate) struct Snapshot {
    pub(crate) health: Option<dual_stack::DualStackReport>,
    pub(crate) physical: Option<dual_stack::DualStackReport>,
    pub(crate) require_physical: bool,
    pub(crate) internet: Vec<internet_probe::InternetProbeOutcome>,
    pub(crate) internet_duration_ms: u128,
    pub(crate) complete: bool,
    pub(crate) cancelled: bool,
    pub(crate) require_session: bool,
    pub(crate) session_checked: bool,
    pub(crate) session_online: Option<bool>,
    pub(crate) session_duration_ms: u128,
}
impl Snapshot {
    /// Internet availability on the default route, including VPN/cellular.
    pub(crate) fn online(&self) -> bool {
        !self.cancelled
            && (self.internet.iter().any(|probe| probe.success)
                || self.health.as_ref().is_some_and(|health| health.online()))
    }
    /// Availability on the network being authenticated. A working VPN default
    /// must never substitute for an unverified Android Wi-Fi/Ethernet Network.
    pub(crate) fn access_online(&self) -> bool {
        !self.cancelled
            && if self.require_physical {
                self.physical.as_ref().is_some_and(|health| health.online())
            } else {
                self.online()
            }
    }
    pub(crate) fn authenticated_online(&self) -> bool {
        self.access_online() && (!self.require_session || self.session_online == Some(true))
    }
}

pub(crate) fn requires_physical_probe(network: &serde_json::Value) -> bool {
    cfg!(target_os = "android")
        && matches!(network["transport"].as_str(), Some("wifi" | "ethernet"))
}

pub(crate) fn same_network(left: &serde_json::Value, right: &serde_json::Value) -> bool {
    [
        "interfaceName",
        "ip",
        "transport",
        "networkId",
        "physicalNetworkHandle",
        "defaultNetworkId",
        "defaultNetworkHandle",
    ]
    .iter()
    .all(|key| left[*key] == right[*key])
}

struct Run {
    id: u64,
    key: String,
    started: Instant,
    sender: watch::Sender<Snapshot>,
}
#[derive(Default)]
struct PoolState {
    next: u64,
    run: Option<Run>,
}
#[derive(Default)]
pub(crate) struct ProbePool(Mutex<PoolState>);

impl ProbePool {
    pub(crate) fn invalidate(&self) -> u64 {
        let mut pool = self.0.lock().unwrap();
        pool.next = pool.next.wrapping_add(1);
        if let Some(run) = pool.run.take() {
            run.sender.send_modify(|value| {
                value.cancelled = true;
                value.complete = true;
            });
        }
        pool.next
    }
    fn subscribe(&self, key: String) -> (u64, watch::Receiver<Snapshot>, bool) {
        let mut pool = self.0.lock().unwrap();
        if let Some(run) = &pool.run {
            // Only share running work or a very recent successful result.
            let value = run.sender.borrow();
            if run.key == key
                && !value.cancelled
                && run.started.elapsed() < Duration::from_secs(4)
                && (!value.complete
                    || (value.authenticated_online()
                        && run.started.elapsed() < Duration::from_secs(2)))
            {
                return (run.id, run.sender.subscribe(), false);
            }
        }
        if let Some(run) = pool.run.take() {
            run.sender.send_modify(|value| {
                value.cancelled = true;
                value.complete = true;
            });
        }
        pool.next = pool.next.wrapping_add(1);
        let id = pool.next;
        let (sender, receiver) = watch::channel(Snapshot::default());
        pool.run = Some(Run {
            id,
            key,
            started: Instant::now(),
            sender,
        });
        (id, receiver, true)
    }
    fn update(&self, id: u64, change: impl FnOnce(&mut Snapshot)) -> bool {
        let pool = self.0.lock().unwrap();
        let Some(run) = pool.run.as_ref().filter(|run| run.id == id) else {
            return false;
        };
        if run.sender.borrow().cancelled {
            return false;
        }
        run.sender.send_modify(change);
        true
    }
    fn cancel(&self, id: u64) {
        self.update(id, |value| {
            value.cancelled = true;
            value.complete = true;
        });
    }
    fn current(&self, id: u64) -> bool {
        self.0
            .lock()
            .unwrap()
            .run
            .as_ref()
            .is_some_and(|run| run.id == id)
    }
}

pub(crate) struct Observation {
    receiver: watch::Receiver<Snapshot>,
}
impl Observation {
    pub(crate) async fn wait_with_updates(mut self, mut update: impl FnMut(&Snapshot)) -> Snapshot {
        loop {
            let value = self.receiver.borrow_and_update().clone();
            update(&value);
            if value.complete || value.cancelled {
                return value;
            }
            if self.receiver.changed().await.is_err() {
                return Snapshot {
                    cancelled: true,
                    complete: true,
                    ..value
                };
            }
        }
    }
    pub(crate) async fn wait(self, full: bool) -> Snapshot {
        self.wait_inner(full || cfg!(target_os = "android"), true)
            .await
    }
    pub(crate) async fn wait_for_login(self) -> Snapshot {
        self.wait_inner(cfg!(target_os = "android"), false).await
    }
    async fn wait_inner(mut self, full: bool, respect_session: bool) -> Snapshot {
        loop {
            let value = self.receiver.borrow_and_update().clone();
            if value.complete
                || value.cancelled
                || (!full
                    && value.access_online()
                    && (!respect_session || !value.require_session || value.session_checked))
            {
                return value;
            }
            if self.receiver.changed().await.is_err() {
                return Snapshot {
                    cancelled: true,
                    complete: true,
                    ..value
                };
            }
        }
    }
}

pub(crate) fn invalidate(app: &tauri::AppHandle, state: &AppState) {
    let probe_id = state.connectivity.invalidate();
    *state.link_health.lock().unwrap() = None;
    let generation = state
        .network_change_generation
        .load(std::sync::atomic::Ordering::SeqCst);
    let _ = app.emit(
        "link-health-reset",
        serde_json::json!({"generation": generation, "probeId": probe_id}),
    );
}

pub(crate) fn observe(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    network: &serde_json::Value,
) -> Observation {
    use std::sync::atomic::Ordering;
    let generation = state.network_change_generation.load(Ordering::SeqCst);
    let compatibility = crate::effective_vpn_compatibility(&state.config.read().unwrap());
    let require_physical = requires_physical_probe(network);
    let adapters = crate::network_inventory::adapters();
    let require_session = network["lgnWiredHint"].as_bool().unwrap_or(false)
        && (!crate::preferred_interface_for_app(app).is_empty()
            || adapters
                .iter()
                .filter(|adapter| {
                    adapter.selectable && adapter.connected && !adapter.ipv4.is_empty()
                })
                .count()
                > 1);
    let key = serde_json::json!([
        generation,
        network["interfaceName"],
        network["ip"],
        network["routeIp"],
        network["transport"],
        network["networkId"],
        network["defaultNetworkId"],
        network["physicalNetworkHandle"],
        network["defaultNetworkHandle"],
        network["captivePortal"],
        network["validated"],
        compatibility.as_str(),
        require_physical,
        require_session
    ])
    .to_string();
    let (id, receiver, start) = state.connectivity.subscribe(key);
    if start {
        state.connectivity.update(id, |value| {
            value.require_session = require_session;
            value.require_physical = require_physical;
        });
        let app = app.clone();
        let state = state.clone();
        let network = network.clone();
        let mut cancellation = receiver.clone();
        tauri::async_runtime::spawn(async move {
            let work = async {
                let public = async {
                    // Android Wi-Fi authentication can bind the process while
                    // default-route probes must still use their explicit Network
                    // handles. Only the per-socket family probes are authoritative.
                    if require_physical {
                        return;
                    }
                    let started = Instant::now();
                    let results =
                        internet_probe::probe_with_updates(network["ip"].as_str(), |outcome| {
                            if generation != state.network_change_generation.load(Ordering::SeqCst)
                            {
                                state.connectivity.cancel(id);
                                return;
                            }
                            state.connectivity.update(id, |value| {
                                if outcome.success {
                                    state.system_online.store(true, Ordering::SeqCst);
                                }
                                value.internet.push(outcome);
                            });
                        })
                        .await;
                    state.connectivity.update(id, |value| {
                        value.internet = results;
                        value.internet_duration_ms = started.elapsed().as_millis();
                    });
                };
                let families = async {
                    dual_stack::probe_with_updates(&network, |health| {
                        if generation != state.network_change_generation.load(Ordering::SeqCst) {
                            state.connectivity.cancel(id);
                            return;
                        }
                        if !state.connectivity.current(id) {
                            return;
                        }
                        let latest = crate::get_network_info(app.clone(), Some(false));
                        if !same_network(&latest, &network) {
                            state.connectivity.cancel(id);
                            return;
                        }
                        let mut health = health.clone();
                        health.generation = generation;
                        health.probe_id = id;
                        state.connectivity.update(id, |value| {
                            value.health = Some(health.clone());
                            *state.link_health.lock().unwrap() = Some(health.clone());
                            if health.online() {
                                state.system_online.store(true, Ordering::SeqCst);
                            }
                            let _ = app.emit("link-health", health);
                        });
                    })
                    .await;
                };
                let physical = async {
                    if !require_physical {
                        return;
                    }
                    dual_stack::probe_route_with_updates(&network, true, |health| {
                        if generation != state.network_change_generation.load(Ordering::SeqCst)
                            || !same_network(
                                &crate::get_network_info(app.clone(), Some(false)),
                                &network,
                            )
                        {
                            state.connectivity.cancel(id);
                            return;
                        }
                        let mut health = health.clone();
                        health.generation = generation;
                        health.probe_id = id;
                        state
                            .connectivity
                            .update(id, |value| value.physical = Some(health));
                    })
                    .await;
                };
                let session = async {
                    let started = Instant::now();
                    let online = if require_session {
                        let route = crate::portal_route_context_from_network(&network)
                            .ok()
                            .flatten();
                        if route.is_some() {
                            tokio::time::timeout(
                                crate::NETWORK_PROBE_TIMEOUT,
                                crate::fetch_portal_user_info(
                                    network["ip"].as_str(),
                                    compatibility,
                                    route.as_ref(),
                                ),
                            )
                            .await
                            .ok()
                            .flatten()
                            .map(|info| !info.account.trim().is_empty())
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    state.connectivity.update(id, |value| {
                        value.session_checked = true;
                        value.session_online = online;
                        value.session_duration_ms = started.elapsed().as_millis();
                    });
                };
                futures_util::future::join(
                    futures_util::future::join(public, families),
                    futures_util::future::join(session, physical),
                )
                .await;
                if generation != state.network_change_generation.load(Ordering::SeqCst) {
                    state.connectivity.cancel(id);
                    return;
                }
                state.connectivity.update(id, |value| {
                    value.complete = true;
                    state.system_online.store(value.online(), Ordering::SeqCst);
                });
            };
            let cancelled = async {
                loop {
                    if cancellation.borrow_and_update().cancelled {
                        break;
                    }
                    if cancellation.changed().await.is_err() {
                        break;
                    }
                }
            };
            // Cancelling the run drops its in-flight read-only HTTP requests.
            futures_util::future::select(Box::pin(work), Box::pin(cancelled)).await;
        });
    }
    Observation { receiver }
}

pub(crate) fn for_app(app: &tauri::AppHandle, network: &serde_json::Value) -> Observation {
    let state = app.state::<Arc<AppState>>();
    observe(app, state.inner(), network)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vpn_cellular_success_cannot_authenticate_the_selected_wifi() {
        let mut system = dual_stack::unavailable("fixture");
        system.ipv4.status = "reachable".into();
        let mut physical = dual_stack::unavailable("fixture");
        physical.scope = "physical".into();
        physical.ipv4.status = "timeout".into();
        physical.ipv6.status = "not_configured".into();
        let mut value = Snapshot {
            health: Some(system),
            physical: Some(physical),
            require_physical: true,
            ..Default::default()
        };
        assert!(
            value.online(),
            "the VPN/mobile default is independently online"
        );
        assert!(!value.authenticated_online());
        assert!(
            !value.access_online(),
            "login verification must also use Wi-Fi"
        );
        value.physical = None;
        assert!(
            !value.authenticated_online(),
            "missing results must not fall back to TUN"
        );
        let mut physical = dual_stack::unavailable("fixture");
        physical.ipv6.status = "reachable".into();
        value.physical = Some(physical);
        assert!(
            value.authenticated_online(),
            "an IPv6-only Wi-Fi result is enough"
        );
        value.health.as_mut().unwrap().ipv4.status = "timeout".into();
        assert!(!value.online());
        assert!(
            value.authenticated_online(),
            "a broken VPN must not hide working Wi-Fi"
        );
        value.cancelled = true;
        assert!(!value.authenticated_online());
    }
    #[test]
    fn changed_android_network_handles_invalidate_same_ip_results() {
        let before = serde_json::json!({"interfaceName":"wlan0","ip":"192.168.1.2","transport":"wifi","networkId":"101","physicalNetworkHandle":"10100","defaultNetworkId":"999","defaultNetworkHandle":"99900"});
        assert!(same_network(&before, &before));
        for key in [
            "networkId",
            "physicalNetworkHandle",
            "defaultNetworkId",
            "defaultNetworkHandle",
        ] {
            let mut after = before.clone();
            after[key] = "changed".into();
            assert!(!same_network(&before, &after), "{key}");
        }
    }
    #[test]
    fn fast_system_probe_waits_for_physical_wifi_result() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let pool = ProbePool::default();
            let (id, receiver, _) = pool.subscribe("vpn-and-captive-wifi".into());
            pool.update(id, |value| {
                value.require_physical = true;
                value.internet.push(internet_probe::InternetProbeOutcome {
                    label: "vpn",
                    success: true,
                    detail: String::new(),
                });
            });
            let mut waiting = Box::pin(Observation { receiver }.wait_inner(false, true));
            assert!(
                tokio::time::timeout(Duration::from_millis(20), &mut waiting)
                    .await
                    .is_err()
            );
            pool.update(id, |value| {
                value.physical = Some(dual_stack::unavailable("Wi-Fi timeout"));
                value.complete = true;
            });
            let result = waiting.await;
            assert!(result.online());
            assert!(!result.authenticated_online());
            assert!(
                pool.subscribe("vpn-and-captive-wifi".into()).2,
                "failed Wi-Fi must be retried"
            );
        });
    }
    #[test]
    fn diagnostics_receive_the_exact_console_snapshot_before_completion() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let pool = ProbePool::default();
            let (id, receiver, _) = pool.subscribe("same-network".into());
            let mut health = dual_stack::unavailable("fixture");
            health.generation = 7;
            health.probe_id = id;
            health.ipv4.status = "reachable".into();
            health.ipv6.status = "checking".into();
            pool.update(id, |value| value.health = Some(health.clone()));
            let mut seen = Vec::new();
            let result = Observation { receiver }
                .wait_with_updates(|snapshot| {
                    seen.push(serde_json::to_value(snapshot.health.as_ref().unwrap()).unwrap());
                    if !snapshot.complete {
                        pool.update(id, |value| {
                            value.health.as_mut().unwrap().ipv6.status = "timeout".into();
                            value.complete = true;
                        });
                    }
                })
                .await;
            assert_eq!(seen.len(), 2);
            assert_eq!(seen[0]["ipv6"]["status"], "checking");
            assert_eq!(
                seen[1],
                serde_json::to_value(result.health.unwrap()).unwrap()
            );
            assert_eq!(seen[1]["generation"], 7);
            assert_eq!(seen[1]["probeId"], id);
        });
    }
    #[test]
    fn foreground_can_finish_while_diagnostics_remain_subscribed() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let pool = ProbePool::default();
            let (id, quick, _) = pool.subscribe("same-route".into());
            let (_, full, start) = pool.subscribe("same-route".into());
            assert!(!start);
            let quick = tokio::spawn(Observation { receiver: quick }.wait(false));
            let full = tokio::spawn(Observation { receiver: full }.wait(true));
            pool.update(id, |value| {
                value.internet.push(internet_probe::InternetProbeOutcome {
                    label: "fast",
                    success: true,
                    detail: String::new(),
                })
            });
            assert!(tokio::time::timeout(Duration::from_secs(1), quick)
                .await
                .unwrap()
                .unwrap()
                .online());
            assert!(!full.is_finished());
            pool.invalidate();
            assert!(
                tokio::time::timeout(Duration::from_secs(1), full)
                    .await
                    .unwrap()
                    .unwrap()
                    .cancelled
            );
        });
    }
    #[test]
    fn online_wifi_does_not_finish_a_required_wired_session_check() {
        // This is also the barrier used by regular checks in a multi-NIC setup.
        let pool = ProbePool::default();
        let (id, receiver, _) = pool.subscribe("two-adapters".into());
        pool.update(id, |value| {
            value.require_session = true;
            value.internet.push(internet_probe::InternetProbeOutcome {
                label: "wifi",
                success: true,
                detail: String::new(),
            });
        });
        assert!(receiver.borrow().online());
        assert!(!receiver.borrow().session_checked);
        pool.update(id, |value| {
            value.session_checked = true;
            value.session_online = Some(false);
        });
        assert_eq!(receiver.borrow().session_online, Some(false));
    }
    #[test]
    fn simultaneous_readers_share_work_but_network_changes_reject_old_updates() {
        let pool = ProbePool::default();
        let (first, old, start) = pool.subscribe("ethernet".into());
        assert!(start);
        let (second, _, start) = pool.subscribe("ethernet".into());
        assert_eq!(first, second);
        assert!(!start);
        let (third, _, start) = pool.subscribe("wifi".into());
        assert!(start);
        assert_ne!(third, first);
        assert!(old.borrow().cancelled);
        assert!(!pool.update(first, |value| value.complete = true));
        pool.invalidate();
        assert!(!pool.current(third));
    }
    #[test]
    fn failed_results_are_retried_and_invalidated_results_cannot_report_online() {
        let pool = ProbePool::default();
        let (id, _, _) = pool.subscribe("network".into());
        pool.update(id, |value| value.complete = true);
        assert!(pool.subscribe("network".into()).2);
        let mut value = Snapshot::default();
        value.internet.push(internet_probe::InternetProbeOutcome {
            label: "fixture",
            success: true,
            detail: String::new(),
        });
        assert!(value.online());
        value.cancelled = true;
        assert!(!value.online());
    }
}
