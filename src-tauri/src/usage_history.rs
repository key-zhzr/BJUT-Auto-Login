//! Sampled billing deltas. Missing intervals and first samples are not usage.
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
}
impl History {
    pub(crate) fn rows(&self) -> Vec<DailyUsage> {
        self.days.values().cloned().collect()
    }
    pub(crate) fn retain_accounts(&mut self, accounts: &[crate::Account]) {
        self.days
            .retain(|_, row| accounts.iter().any(|a| a.user == row.account));
        // Baselines are opaque composite keys; bound them independently.
        if self.baselines.len() > 5000 {
            self.baselines.clear();
        }
    }
    pub(crate) fn observe(
        &mut self,
        account: &str,
        at: i64,
        devices: &[crate::billing::BillingOnlineSession],
    ) {
        let Some(date) = chrono::DateTime::from_timestamp(at, 0)
            .map(|d| (d + chrono::Duration::hours(8)).date_naive())
        else {
            return;
        };
        let day = date.to_string();
        let oldest = (date - chrono::Duration::days(120)).to_string();
        self.days.retain(|_, row| row.day >= oldest);
        for device in devices {
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
            let session =
                serde_json::to_string(&(account, &identity, &device.ip, &device.login_at)).unwrap();
            let previous = self.baselines.get(&session);
            let delta = previous
                .filter(|p| at > p.at && at - p.at <= 180 && used >= p.used)
                .map(|p| (used - p.used, at - p.at));
            // Retain a same-session high water mark when a stale server reply
            // temporarily lowers its counter. This prevents double counting.
            let high = previous.map_or(used, |p| p.used.max(used));
            self.baselines.insert(session, Baseline { at, used: high });
            if let Some((delta, seconds)) = delta {
                let key = serde_json::to_string(&(&day, account, &identity)).unwrap();
                let row = self.days.entry(key).or_insert_with(|| DailyUsage {
                    day: day.clone(),
                    account: account.into(),
                    device: identity.clone(),
                    label: if device.mac.is_empty() {
                        device.ip.clone()
                    } else {
                        device.mac.clone()
                    },
                    ..Default::default()
                });
                row.used_mb += delta;
                row.sampled_seconds += seconds;
            }
        }
        // Hard limits also bound local-file size when many accounts churn.
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
    fn device(used: &str, login: &str) -> crate::billing::BillingOnlineSession {
        crate::billing::BillingOnlineSession {
            login_at: login.into(),
            ip: "192.0.2.1".into(),
            ipv6: String::new(),
            mac: "AA-BB-CC-DD-EE-FF".into(),
            duration_minutes: "1".into(),
            used_flow_mb: used.into(),
            session_id: "unused".into(),
        }
    }
    #[test]
    fn samples_ignore_gaps_resets_and_first_counter() {
        let mut history = History::default();
        let t = 1792886400;
        history.observe("one", t, &[device("100", "first")]);
        assert!(history.rows().is_empty());
        history.observe("one", t + 60, &[device("110", "first")]);
        assert_eq!(history.rows()[0].used_mb, 10.);
        history.observe("one", t + 120, &[device("90", "first")]);
        history.observe("one", t + 180, &[device("112", "first")]);
        assert_eq!(history.rows()[0].used_mb, 12.);
        history.observe("one", t + 600, &[device("200", "first")]);
        assert_eq!(history.rows()[0].used_mb, 12.);
        history.observe("one", t + 660, &[device("500", "new-session")]);
        assert_eq!(history.rows()[0].used_mb, 12.);
    }
    #[test]
    fn restored_baselines_and_accounts_stay_separate() {
        let mut h = History::default();
        let t = 1792886400;
        h.observe("a", t, &[device("10", "first")]);
        let mut restored: History =
            serde_json::from_slice(&serde_json::to_vec(&h).unwrap()).unwrap();
        restored.observe("a", t + 60, &[device("11", "first")]);
        restored.observe("b", t + 60, &[device("11", "first")]);
        assert_eq!(restored.rows().len(), 1);
    }
}
