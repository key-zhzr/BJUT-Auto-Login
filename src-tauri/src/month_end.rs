//! Conservative, read-only renewal checks. Never infer a monthly fee from an
//! unrelated number (flow allowance, yearly pricing or a per-GB rate).
use chrono::{Datelike, NaiveDate};

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct CheckStamp {
    pub(crate) attempted_at: i64,
    pub(crate) checked_day: String,
}
pub(crate) fn read_stamp(path: &std::path::Path) -> Option<CheckStamp> {
    if std::fs::metadata(path).ok()?.len() > 1024 {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}
pub(crate) fn save_stamp(path: &std::path::Path, stamp: CheckStamp) {
    let write = || -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("tmp");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec(&stamp)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(temporary, path)?;
        Ok(())
    };
    // A local metadata write failure must never interrupt a login or payment.
    let _ = write();
}

pub(crate) fn in_check_window(today: NaiveDate) -> bool {
    let (year, month) = if today.month() == 12 {
        (today.year() + 1, 1)
    } else {
        (today.year(), today.month() + 1)
    };
    NaiveDate::from_ymd_opt(year, month, 1)
        .is_some_and(|next| (1..=5).contains(&(next - today).num_days()))
}

// Ten-thousandths of a yuan retain the precision returned by jfself/ydapp.
pub(crate) fn money(value: &str) -> Option<i64> {
    let text = value
        .trim()
        .trim_start_matches(['¥', '￥'])
        .trim_end_matches('元')
        .trim();
    let (negative, text) = match text.strip_prefix('-') {
        Some(text) => (true, text),
        None => (false, text),
    };
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > 4
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let amount = whole
        .parse::<i64>()
        .ok()?
        .checked_mul(10_000)?
        .checked_add(if fraction.is_empty() {
            0
        } else {
            fraction
                .parse::<i64>()
                .ok()?
                .checked_mul(10_i64.pow(4 - fraction.len() as u32))?
        })?;
    Some(if negative { -amount } else { amount })
}

fn amounts(text: &str, name: bool) -> Vec<i64> {
    if name
        && ["年", "季度", "季卡", "日租", "小时", "按次", "/GB", "/MB"]
            .iter()
            .any(|word| text.contains(word))
    {
        return Vec::new();
    }
    text.match_indices('元')
        .filter_map(|(at, _)| {
            let before = &text[..at];
            let after = &text[at + '元'.len_utf8()..];
            if ["/年", "/GB", "/MB", "/小时", "/日", "/次", "／年"]
                .iter()
                .any(|unit| after.trim_start().starts_with(unit))
            {
                return None;
            }
            let raw: String = before
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit() || *c == '.' || c.is_whitespace())
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            let nearby: String = before
                .chars()
                .rev()
                .take(16)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            let monthly = after.trim_start().starts_with("/月")
                || after.trim_start().starts_with("／月")
                || ["月租", "包月", "每月", "基本费"]
                    .iter()
                    .any(|word| nearby.contains(word));
            if !(monthly || name && text.contains("套餐")) {
                return None;
            }
            money(&raw).filter(|amount| *amount >= 0)
        })
        .collect()
}

pub(crate) fn monthly_fee(name: &str, detail: &str) -> Option<i64> {
    let mut values = amounts(detail, false);
    if values.is_empty() {
        values = amounts(name, true);
    }
    values.sort();
    values.dedup();
    (values.len() == 1).then(|| values[0])
}

pub(crate) fn insufficient(balance: &str, name: &str, detail: &str) -> Option<(i64, i64)> {
    let balance = money(balance)?;
    let fee = monthly_fee(name, detail)?;
    (balance < fee).then_some((balance, fee))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn check_stamp_survives_restart_and_can_be_updated() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("bjut-month-end-{}-{unique}", std::process::id()));
        let path = directory.join("check.json");
        assert!(read_stamp(&path).is_none());
        save_stamp(
            &path,
            CheckStamp {
                attempted_at: 100,
                checked_day: String::new(),
            },
        );
        assert_eq!(read_stamp(&path).unwrap().attempted_at, 100);
        save_stamp(
            &path,
            CheckStamp {
                attempted_at: 200,
                checked_day: "2026-09-26".into(),
            },
        );
        let restored = read_stamp(&path).unwrap();
        assert_eq!(restored.attempted_at, 200);
        assert_eq!(restored.checked_day, "2026-09-26");
        std::fs::write(&path, vec![b' '; 1025]).unwrap();
        assert!(read_stamp(&path).is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn final_five_days_include_leap_year_and_year_rollover() {
        for (value, expected) in [
            ("2026-02-23", false),
            ("2026-02-24", true),
            ("2028-02-24", false),
            ("2028-02-25", true),
            ("2026-12-27", true),
            ("2027-01-01", false),
        ] {
            assert_eq!(in_check_window(value.parse().unwrap()), expected, "{value}");
        }
    }
    #[test]
    fn fees_come_from_monthly_prices_not_traffic_or_annual_plans() {
        assert_eq!(monthly_fee("本科生10元套餐", ""), Some(100_000));
        assert_eq!(
            monthly_fee("研究生套餐", "含 100GB，月租 20 元/月"),
            Some(200_000)
        );
        assert_eq!(monthly_fee("免费0元套餐", ""), Some(0));
        assert_eq!(
            monthly_fee("校园套餐", "月租10元，超出流量0.2元/GB"),
            Some(100_000)
        );
        assert_eq!(monthly_fee("校园套餐", "超出流量 1 元/GB"), None);
        assert_eq!(monthly_fee("100元年套餐", ""), None);
        assert_eq!(monthly_fee("未知套餐", ""), None);
        assert_eq!(monthly_fee("套餐", "月租10元或月租20元"), None);
    }
    #[test]
    fn negative_balances_and_exact_fee_boundaries_are_preserved() {
        assert_eq!(money("-0.0001 元"), Some(-1));
        assert_eq!(money("￥10.50"), Some(105_000));
        assert_eq!(money("--"), None);
        assert!(insufficient("10.0000", "10元套餐", "").is_none());
        assert!(insufficient("9.9999", "10元套餐", "").is_some());
        assert!(insufficient("100", "未知套餐", "").is_none());
    }
}
