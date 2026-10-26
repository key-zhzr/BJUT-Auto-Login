//! Billing counter deltas with bounded minute/hour/day rollups. Missing samples
//! are gaps; first samples, disabled intervals and counter resets are not usage.
use std::collections::BTreeMap;
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DailyUsage {
    pub day: String,
    pub account: String,
    pub device: String,
    pub label: String,
    pub used_mb: f64,
    pub sampled_seconds: i64,
    #[serde(default)]
    pub local: bool,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsagePoint {
    pub at: i64,
    pub resolution: i64,
    pub account: String,
    pub device: String,
    pub interface_id: String,
    pub interface_name: String,
    pub local: bool,
    pub used_mb: f64,
    pub sampled_seconds: i64,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Baseline {
    at: i64,
    used: f64,
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct History {
    baselines: BTreeMap<String, Baseline>,
    days: BTreeMap<String, DailyUsage>,
    #[serde(default)]
    series: Vec<UsagePoint>,
}
impl History {
    pub(crate) fn pause(&mut self) {
        self.baselines.clear();
    }
    pub(crate) fn rows(&self) -> Vec<DailyUsage> {
        self.days.values().cloned().collect()
    }
    pub(crate) fn points(&self) -> Vec<UsagePoint> {
        self.series.clone()
    }
    pub(crate) fn retain_accounts(&mut self, accounts: &[crate::Account]) {
        self.days
            .retain(|_, row| accounts.iter().any(|a| a.user == row.account));
        self.series
            .retain(|row| accounts.iter().any(|a| a.user == row.account));
        if self.baselines.len() > 5000 {
            self.baselines.clear();
        }
    }
    pub(crate) fn observe(
        &mut self,
        account: &str,
        at: i64,
        devices: &[crate::device_usage::UsageDevice],
    ) {
        let Some(date) = chrono::DateTime::from_timestamp(at, 0)
            .map(|d| (d + chrono::Duration::hours(8)).date_naive())
        else {
            return;
        };
        let day = date.to_string();
        let oldest = (date - chrono::Duration::days(120)).to_string();
        self.days.retain(|_, row| row.day >= oldest);
        for observation in devices {
            let device = &observation.device;
            let Ok(used) = device.used_flow_mb.parse::<f64>() else {
                continue;
            };
            if !used.is_finite() || used < 0. {
                continue;
            }
            let identity = if device.mac.is_empty() {
                device.ip.clone()
            } else {
                device.mac.replace([':', '-'], "").to_ascii_lowercase()
            };
            let session = serde_json::to_string(&(
                account,
                &identity,
                &device.ip,
                &device.login_at,
                &observation.interface_id,
            ))
            .unwrap();
            let previous = self.baselines.get(&session);
            let delta = previous
                .filter(|p| at > p.at && at - p.at <= 180 && used >= p.used)
                .map(|p| (used - p.used, at - p.at));
            // Ignore a stale lower counter without counting its recovery twice.
            let high = previous.map_or(used, |p| p.used.max(used));
            self.baselines.insert(session, Baseline { at, used: high });
            if let Some((delta, seconds)) = delta {
                let key =
                    serde_json::to_string(&(&day, account, &identity, &observation.interface_id))
                        .unwrap();
                let row = self.days.entry(key).or_insert_with(|| DailyUsage {
                    day: day.clone(),
                    account: account.into(),
                    device: identity.clone(),
                    label: observation.interface_name.clone(),
                    local: observation.local,
                    ..Default::default()
                });
                row.used_mb += delta;
                row.sampled_seconds += seconds;
                for resolution in [60, 3600, 86400] {
                    let bucket = (at + 28800).div_euclid(resolution) * resolution - 28800;
                    if let Some(row) = self.series.iter_mut().rev().find(|row| {
                        row.at == bucket
                            && row.resolution == resolution
                            && row.account == account
                            && row.device == identity
                            && row.interface_id == observation.interface_id
                    }) {
                        row.used_mb += delta;
                        row.sampled_seconds += seconds;
                    } else {
                        self.series.push(UsagePoint {
                            at: bucket,
                            resolution,
                            account: account.into(),
                            device: identity.clone(),
                            interface_id: observation.interface_id.clone(),
                            interface_name: observation.interface_name.clone(),
                            local: observation.local,
                            used_mb: delta,
                            sampled_seconds: seconds,
                        });
                    }
                }
            }
        }
        self.series.retain(|row| {
            at - row.at
                <= match row.resolution {
                    60 => 172800,
                    3600 => 7776000,
                    _ => 31622400,
                }
        });
        self.series.sort_by_key(|row| std::cmp::Reverse(row.at));
        let mut counts = [0; 3];
        self.series.retain(|row| {
            let tier = match row.resolution {
                60 => 0,
                3600 => 1,
                _ => 2,
            };
            counts[tier] += 1;
            counts[tier] <= [12000, 6000, 1800][tier]
        });
        while self.days.len() > 10_000 {
            if let Some(key) = self.days.keys().next().cloned() {
                self.days.remove(&key);
            }
        }
        if self.baselines.len() > 5000 {
            self.baselines.clear();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn device(used: &str, login: &str) -> crate::device_usage::UsageDevice {
        crate::device_usage::UsageDevice {
            device: crate::billing::BillingOnlineSession {
                login_at: login.into(),
                ip: "192.0.2.1".into(),
                ipv6: String::new(),
                mac: "AA-BB-CC-DD-EE-FF".into(),
                duration_minutes: "1".into(),
                used_flow_mb: used.into(),
                session_id: "unused".into(),
            },
            interface_id: "en7".into(),
            interface_name: "USB LAN".into(),
            local: true,
        }
    }
    #[test]
    fn samples_ignore_gaps_resets_first_counter_and_paused_time() {
        let mut h = History::default();
        let t = 1792886400;
        h.observe("a", t, &[device("100", "first")]);
        assert!(h.rows().is_empty());
        h.observe("a", t + 60, &[device("110", "first")]);
        h.observe("a", t + 120, &[device("90", "first")]);
        h.observe("a", t + 180, &[device("112", "first")]);
        assert_eq!(h.rows()[0].used_mb, 12.);
        h.observe("a", t + 600, &[device("200", "first")]);
        h.observe("a", t + 660, &[device("500", "new")]);
        assert_eq!(h.rows()[0].used_mb, 12.);
        h.pause();
        h.observe("a", t + 720, &[device("600", "new")]);
        assert_eq!(h.rows()[0].used_mb, 12.);
        for resolution in [60, 3600, 86400] {
            assert_eq!(
                h.points()
                    .iter()
                    .filter(|p| p.resolution == resolution)
                    .map(|p| p.used_mb)
                    .sum::<f64>(),
                12.
            );
        }
    }
    #[test]
    fn restored_rollups_separate_accounts_and_keep_interface_identity() {
        let mut h = History::default();
        let t = 1792886400;
        h.observe("a", t, &[device("10", "first")]);
        let mut restored: History =
            serde_json::from_slice(&serde_json::to_vec(&h).unwrap()).unwrap();
        restored.observe("a", t + 60, &[device("11", "first")]);
        restored.observe("b", t + 60, &[device("11", "first")]);
        assert_eq!(restored.rows().len(), 1);
        assert_eq!(restored.points().len(), 3);
        assert!(restored
            .points()
            .iter()
            .all(|p| p.local && p.interface_id == "en7"));
        let mut moved = device("20", "first");
        moved.interface_id = "en0".into();
        restored.observe("a", t + 120, &[moved]);
        assert_eq!(restored.rows()[0].used_mb, 1.);
    }
}
