use crate::portal_auth::LoginType;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NetworkTrustDecision {
    Allowed,
    Blocked,
    NeedsConfirmation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NetworkTrustResult {
    pub decision: NetworkTrustDecision,
    pub reason: String,
    pub network_key: String,
}

pub(crate) struct NetworkTrustInput<'a> {
    pub login_type: &'a LoginType,
    pub ssid: &'a str,
    pub bssid: &'a str,
    pub ip: &'a str,
    pub transport: &'a str,
    pub identity_fresh: bool,
    pub whitelist: &'a [String],
    pub blacklist: &'a [String],
}

pub(crate) fn normalize_ssid(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase().replace('_', "-");
    let mut collapsed = String::with_capacity(normalized.len());
    let mut previous_was_separator = false;
    for character in normalized.chars() {
        if character == '-' {
            if !previous_was_separator {
                collapsed.push(character);
            }
            previous_was_separator = true;
        } else {
            collapsed.push(character);
            previous_was_separator = false;
        }
    }
    collapsed
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CampusWifiKind {
    Dormitory,
    Public,
}

fn valid_dorm_ap_id(value: &str) -> bool {
    value.len() == 4
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

fn has_observed_dorm_suffix(value: &str, prefix: &str, allow_bare: bool) -> bool {
    let Some(suffix) = value.strip_prefix(prefix) else {
        return false;
    };
    if suffix.is_empty() {
        return allow_bare;
    }
    // The measured AP identifier is consistently four ASCII letters/digits.
    // Requiring the complete suffix avoids turning a familiar-looking personal
    // hotspot such as `bjut-sushe-private` into a trusted campus identity.
    ["-2.4g-", "-5g-", "2.4g-", "5g-", "-2.4-"]
        .iter()
        .any(|marker| suffix.strip_prefix(marker).is_some_and(valid_dorm_ap_id))
}

pub(crate) fn campus_wifi_kind(ssid: &str) -> Option<CampusWifiKind> {
    let normalized = normalize_ssid(ssid);
    if normalized == "bjut-wifi" {
        return Some(CampusWifiKind::Public);
    }

    if has_observed_dorm_suffix(&normalized, "bjut-sushe", true)
        || has_observed_dorm_suffix(&normalized, "bjutsushe", false)
    {
        return Some(CampusWifiKind::Dormitory);
    }

    // These exact spelling variants were present in the supplied multi-floor
    // scans. Keep the list explicit instead of using edit distance, which could
    // classify unrelated personal hotspots as a campus network.
    [
        "bj-sushe",
        "bjit-sushe",
        "bjur-sushe",
        "bjut-suahe",
        "bjut-sudhe",
        "bjut-suhe",
        "bjut-sushr",
        "bjuy-sushe",
        "bnut-sushe",
    ]
    .iter()
    .any(|prefix| has_observed_dorm_suffix(&normalized, prefix, false))
    .then_some(CampusWifiKind::Dormitory)
}

fn normalize_bssid(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', ":")
}

pub(crate) fn network_key(ssid: &str, bssid: &str) -> String {
    format!("{}|{}", normalize_ssid(ssid), normalize_bssid(bssid))
}

fn canonicalize_saved_key(value: &str) -> String {
    let (ssid, bssid) = value.split_once('|').unwrap_or((value, ""));
    network_key(ssid, bssid)
}

fn list_contains(list: &[String], key: &str) -> bool {
    list.iter()
        .any(|entry| canonicalize_saved_key(entry) == key)
}

pub(crate) fn normalize_trust_lists(whitelist: &mut Vec<String>, blacklist: &mut Vec<String>) {
    *blacklist = blacklist
        .iter()
        .map(|entry| canonicalize_saved_key(entry))
        .filter(|entry| entry != "|")
        .collect();
    blacklist.sort();
    blacklist.dedup();

    *whitelist = whitelist
        .iter()
        .map(|entry| canonicalize_saved_key(entry))
        .filter(|entry| entry != "|" && !blacklist.contains(entry))
        .collect();
    whitelist.sort();
    whitelist.dedup();
}

pub(crate) fn set_network_trust(
    whitelist: &mut Vec<String>,
    blacklist: &mut Vec<String>,
    ssid: &str,
    bssid: &str,
    trusted: bool,
) -> String {
    let key = network_key(ssid, bssid);
    whitelist.retain(|entry| canonicalize_saved_key(entry) != key);
    blacklist.retain(|entry| canonicalize_saved_key(entry) != key);
    if trusted {
        whitelist.push(key.clone());
    } else {
        blacklist.push(key.clone());
    }
    normalize_trust_lists(whitelist, blacklist);
    key
}

pub(crate) fn is_campus_local_ip(ip: &str) -> bool {
    let octets: Vec<u8> = ip
        .split('.')
        .map(str::parse::<u8>)
        .collect::<Result<_, _>>()
        .unwrap_or_default();
    if octets.len() != 4 {
        return false;
    }
    matches!(
        (octets[0], octets[1]),
        (10, 17..=27) | (10, 121) | (10, 126) | (10, 226) | (172, 17..=27)
    )
}

pub(crate) fn is_known_campus_ssid(ssid: &str) -> bool {
    campus_wifi_kind(ssid).is_some()
}

pub(crate) fn evaluate_network_trust(input: NetworkTrustInput<'_>) -> NetworkTrustResult {
    let NetworkTrustInput {
        login_type,
        ssid,
        bssid,
        ip,
        transport,
        identity_fresh,
        whitelist,
        blacklist,
    } = input;
    let normalized_ssid = normalize_ssid(ssid);
    let key = network_key(ssid, bssid);
    if list_contains(blacklist, &key) {
        return NetworkTrustResult {
            decision: NetworkTrustDecision::Blocked,
            reason: format!("当前网络 ({}) 在黑名单中", ssid.trim()),
            network_key: key,
        };
    }
    if transport.eq_ignore_ascii_case("wifi") && !identity_fresh {
        return NetworkTrustResult {
            decision: NetworkTrustDecision::Blocked,
            reason: "无法取得当前 Wi-Fi 的新鲜 SSID/BSSID，已阻止发送账号密码".to_string(),
            network_key: key,
        };
    }
    if list_contains(whitelist, &key) {
        return NetworkTrustResult {
            decision: NetworkTrustDecision::Allowed,
            reason: "当前网络已加入白名单".to_string(),
            network_key: key,
        };
    }
    if !is_campus_local_ip(ip) {
        return NetworkTrustResult {
            decision: NetworkTrustDecision::NeedsConfirmation,
            reason: "本地 IP 不属于已知校园网网段".to_string(),
            network_key: key,
        };
    }

    let allowed = match login_type {
        LoginType::Type1 => campus_wifi_kind(&normalized_ssid) == Some(CampusWifiKind::Dormitory),
        LoginType::Type2 => campus_wifi_kind(&normalized_ssid) == Some(CampusWifiKind::Public),
        LoginType::Type3 => {
            (transport.is_empty()
                || transport.eq_ignore_ascii_case("unknown")
                || transport.eq_ignore_ascii_case("ethernet"))
                && (normalized_ssid.is_empty()
                    || normalized_ssid == "unknown"
                    || normalized_ssid == "<unknown ssid>")
        }
        LoginType::Unknown => false,
    };
    NetworkTrustResult {
        decision: if allowed {
            NetworkTrustDecision::Allowed
        } else {
            NetworkTrustDecision::NeedsConfirmation
        },
        reason: if allowed {
            "当前网络符合校园网身份规则".to_string()
        } else if *login_type == LoginType::Type3 {
            "lgn 协议默认仅允许有线网络；当前网络需要明确确认".to_string()
        } else {
            "无线网络名称未经识别；当前网络需要明确确认".to_string()
        },
        network_key: key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blacklist_wins_and_lists_cannot_conflict() {
        let mut whitelist = vec!["BJUT_WIFI|AA-BB".to_string()];
        let mut blacklist = vec!["bjut-wifi|aa:bb".to_string()];
        normalize_trust_lists(&mut whitelist, &mut blacklist);
        assert!(whitelist.is_empty());
        assert_eq!(blacklist, vec!["bjut-wifi|aa:bb"]);
    }

    #[test]
    fn campus_ssid_matching_uses_scan_derived_variants_without_broad_fuzzy_matching() {
        assert_eq!(
            campus_wifi_kind("bjut-sushe-5G-24vF"),
            Some(CampusWifiKind::Dormitory)
        );
        assert_eq!(
            campus_wifi_kind("bjut-sushe--2.4G-eXd2"),
            Some(CampusWifiKind::Dormitory)
        );
        assert_eq!(
            campus_wifi_kind("bjut_sushe-5G-69Xe"),
            Some(CampusWifiKind::Dormitory)
        );
        assert_eq!(
            campus_wifi_kind("bjutsushe-5G-aU97"),
            Some(CampusWifiKind::Dormitory)
        );
        assert_eq!(
            campus_wifi_kind("bjut-sushe2.4G-a8Gb"),
            Some(CampusWifiKind::Dormitory)
        );
        assert_eq!(
            campus_wifi_kind("bjut-sushe-2.4-5JqM"),
            Some(CampusWifiKind::Dormitory)
        );
        assert_eq!(
            campus_wifi_kind("bjut_sushe"),
            Some(CampusWifiKind::Dormitory)
        );
        for observed_typo in [
            "bj-sushe-5G-a8Gb",
            "bjit-sushe-2.4G-e8F6",
            "bjur-sushe-5G-eMs6",
            "bjut-suahe-5G-6Y6m",
            "bjut-sudhe-2.4G-p5X9",
            "bjut-suhe-2.4G-7XEm",
            "bjut-sushr-2.4G-yB55",
            "bjuy-sushe-5G-V9ye",
            "bnut-sushe-5G-A3Mw",
        ] {
            assert_eq!(
                campus_wifi_kind(observed_typo),
                Some(CampusWifiKind::Dormitory),
                "{observed_typo}"
            );
        }
        assert!(is_known_campus_ssid("BJUT_WIFI"));
        for unrelated in [
            "room-bjut-sushe-5g",
            "CU_bjut-sushe-28Au",
            "not_bjut_wifi",
            "evil-bjut-wifi",
            "BJUT-OLY",
            "bjut.wife",
            "bjut-suahe-private",
            "bjut-sushe-private",
            "bjut-susheff",
            "bjut-susheushe-2.4G-Bx34",
            "bjut-sushe-438",
            "bjut-sushe-R8",
            "bjut_wifi_ceshi",
        ] {
            assert!(!is_known_campus_ssid(unrelated), "{unrelated}");
        }
    }

    #[test]
    fn stale_wifi_identity_is_never_accepted_for_credentials() {
        let result = evaluate_network_trust(NetworkTrustInput {
            login_type: &LoginType::Type2,
            ssid: "bjut_wifi",
            bssid: "aa:bb",
            ip: "10.21.1.2",
            transport: "wifi",
            identity_fresh: false,
            whitelist: &["bjut-wifi|aa:bb".to_string()],
            blacklist: &[],
        });
        assert_eq!(result.decision, NetworkTrustDecision::Blocked);
    }
}
