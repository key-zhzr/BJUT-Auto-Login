//! Billing counter deltas with bounded minute/hour/day rollups. Missing samples
//! are gaps; first samples, disabled intervals and counter resets are not usage.
use std::collections::BTreeMap;
fn timestamp_absent(value: &i64) -> bool {
    *value <= 0
}
#[derive(Clone, serde::Deserialize)]
pub(crate) struct Query {
    pub start: i64,
    pub end: i64,
    pub resolution: i64,
    #[serde(default)]
    pub monthly: bool,
}
impl Query {
    pub fn accepts(&self, row: &UsagePoint) -> bool {
        let bucket = |at: i64, res: i64| (at + 28800).div_euclid(res) * res - 28800;
        if ![60, 3600, 86400].contains(&self.resolution) || self.end < self.start {
            return false;
        }
        if row.resolution == self.resolution
            && row.at >= bucket(self.start, self.resolution)
            && row.at <= self.end
        {
            return true;
        }
        if !self.monthly || row.at >= self.start {
            return false;
        }
        use chrono::Datelike;
        let Some(date) = chrono::DateTime::from_timestamp(self.start + 28800, 0) else {
            return false;
        };
        let Some(month) = date
            .with_day(1)
            .and_then(|d| d.date_naive().and_hms_opt(0, 0, 0))
            .map(|d| d.and_utc().timestamp() - 28800)
        else {
            return false;
        };
        if row.at < month {
            return false;
        }
        let day = bucket(self.start, 86400);
        let hour = bucket(self.start, 3600);
        (row.resolution == 86400 && row.at < day)
            || (row.resolution == 3600 && row.at >= day && row.at < hour)
            || (row.resolution == 60 && row.at >= hour)
    }
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Retention {
    pub minute_hours: u32,
    pub hour_days: u32,
    pub day_days: u32,
    pub storage_mb: u32,
}
impl Default for Retention {
    fn default() -> Self {
        Self {
            minute_hours: 48,
            hour_days: 90,
            day_days: 366,
            storage_mb: 64,
        }
    }
}
impl Retention {
    pub fn normalize(&mut self) {
        self.minute_hours = self.minute_hours.clamp(1, 8760);
        self.hour_days = self.hour_days.clamp(1, 3660);
        self.day_days = self.day_days.clamp(1, 36600);
        self.storage_mb = self.storage_mb.clamp(1, 128);
    }
}
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
    #[serde(default, skip_serializing_if = "timestamp_absent")]
    pub started_at: i64,
    #[serde(default, skip_serializing_if = "timestamp_absent")]
    pub ended_at: i64,
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
    #[cfg(test)]
    pub(crate) fn observe(
        &mut self,
        account: &str,
        at: i64,
        devices: &[crate::device_usage::UsageDevice],
    ) {
        self.observe_with_retention(account, at, devices, &Retention::default());
    }
    pub(crate) fn observe_with_retention(
        &mut self,
        account: &str,
        at: i64,
        devices: &[crate::device_usage::UsageDevice],
        retention: &Retention,
    ) {
        let Some(date) = chrono::DateTime::from_timestamp(at, 0)
            .map(|d| (d + chrono::Duration::hours(8)).date_naive())
        else {
            return;
        };
        let day = date.to_string();
        let oldest = (date - chrono::Duration::days(i64::from(retention.day_days))).to_string();
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
                        if row.started_at == 0 {
                            row.started_at = at - seconds;
                        } else {
                            row.started_at = row.started_at.min(at - seconds);
                        }
                        row.ended_at = at;
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
                            started_at: at - seconds,
                            ended_at: at,
                        });
                    }
                }
            }
        }
        self.series.retain(|row| {
            at - row.at
                <= match row.resolution {
                    60 => i64::from(retention.minute_hours) * 3600,
                    3600 => i64::from(retention.hour_days) * 86400,
                    _ => i64::from(retention.day_days) * 86400,
                }
        });
        self.series.sort_by_key(|row| std::cmp::Reverse(row.at));
        let mut counts = [0; 3];
        let capacity = (retention.storage_mb as usize * 1024 * 1024 / 400).max(100);
        self.series.retain(|row| {
            let tier = match row.resolution {
                60 => 0,
                3600 => 1,
                _ => 2,
            };
            counts[tier] += 1;
            counts[tier] <= [capacity * 6 / 10, capacity * 3 / 10, capacity / 10][tier]
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
    #[test]
    fn real_intervals_and_retention_survive_unrelated_settings() {
        let mut h = History::default();
        let t = 1792886459;
        h.observe("a", t, &[device("10", "same")]);
        let retention = Retention {
            minute_hours: 24,
            ..Default::default()
        };
        h.observe_with_retention("a", t + 61, &[device("12", "same")], &retention);
        let minute = h.points().into_iter().find(|p| p.resolution == 60).unwrap();
        assert_eq!(
            (minute.started_at, minute.ended_at, minute.sampled_seconds),
            (t, t + 61, 61)
        );
        assert_eq!(minute.used_mb, 2.);
        let encoded = serde_json::to_string(&minute).unwrap();
        assert!(encoded.contains("startedAt"));
        let old = encoded
            .replace(&format!(",\"startedAt\":{}", t), "")
            .replace(&format!(",\"endedAt\":{}", t + 61), "");
        assert_eq!(
            serde_json::from_str::<UsagePoint>(&old).unwrap().started_at,
            0
        );
    }
    #[test]
    fn monthly_query_includes_disjoint_prefix_tiers() {
        let t = 1792886459;
        let mut h = History::default();
        h.observe("a", t, &[device("10", "same")]);
        h.observe("a", t + 61, &[device("12", "same")]);
        let q = Query {
            start: t + 70,
            end: t + 3600,
            resolution: 60,
            monthly: true,
        };
        let rows = h
            .points()
            .into_iter()
            .filter(|p| q.accepts(p))
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].resolution, 60);
        let invalid = Query { resolution: 1, ..q };
        assert!(!invalid.accepts(&rows[0]));
    }
}
