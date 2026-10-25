use crate::{device_usage::AccountUsage, Account};
use std::collections::HashMap;
fn score(sample: Option<&AccountUsage>, now: i64) -> f64 {
    let Some(sample) =
        sample.filter(|s| s.error.is_none() && now >= s.updated_at && now - s.updated_at <= 180)
    else {
        return -1.;
    };
    if ["不限", "无限", "unlimited"]
        .iter()
        .any(|text| sample.remaining_flow.to_ascii_lowercase().contains(text))
    {
        return f64::MAX;
    }
    let text = sample.remaining_flow.trim();
    let mut split = text.split_whitespace();
    let number = split.next().and_then(|v| v.parse::<f64>().ok());
    let scale = match split.next().unwrap_or("MB").to_ascii_uppercase().as_str() {
        "GB" => 1024.,
        "TB" => 1048576.,
        "MB" => 1.,
        _ => return -1.,
    };
    number
        .filter(|v| v.is_finite() && *v >= 0.)
        .map(|v| if v == 0. { -2. } else { v * scale })
        .unwrap_or(-1.)
}
pub(crate) fn order(
    mut accounts: Vec<Account>,
    samples: &HashMap<String, AccountUsage>,
    now: i64,
) -> Vec<Account> {
    accounts.sort_by(|a, b| {
        score(samples.get(&b.user), now).total_cmp(&score(samples.get(&a.user), now))
    });
    accounts
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_and_unknown_data_dont_outrank_fresh_positive_or_starve_fallback() {
        let rows = [
            ("zero", "0 MB", 1000),
            ("unknown", "--", 1000),
            ("stale", "999 GB", 1),
            ("fresh", "1 GB", 1000),
        ];
        let accounts = rows
            .iter()
            .map(|(user, _, _)| Account {
                user: (*user).into(),
                pass: "x".into(),
                is_default: false,
                is_disabled: None,
            })
            .collect();
        let samples = rows
            .iter()
            .map(|(u, f, t)| {
                (
                    (*u).into(),
                    AccountUsage {
                        account: (*u).into(),
                        remaining_flow: (*f).into(),
                        updated_at: *t,
                        ..Default::default()
                    },
                )
            })
            .collect();
        assert_eq!(
            order(accounts, &samples, 1060)
                .iter()
                .map(|a| a.user.as_str())
                .collect::<Vec<_>>(),
            vec!["fresh", "unknown", "stale", "zero"]
        );
    }
}
