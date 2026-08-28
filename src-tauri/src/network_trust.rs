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

fn has_china_unicom_dorm_suffix(value: &str) -> bool {
    let Some(suffix) = value.strip_prefix("cu-bjut-sushe-") else {
        return false;
    };
    let ap_id = suffix.strip_suffix("-5g").unwrap_or(suffix);
    valid_dorm_ap_id(ap_id)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DormitorySsidProfile {
    CampusInfrastructure,
    ManagedRouter,
    ChinaUnicomRouter,
}

fn dormitory_ssid_profile(normalized: &str) -> Option<DormitorySsidProfile> {
    if normalized == "bjut-sushe" {
        return Some(DormitorySsidProfile::CampusInfrastructure);
    }
    if has_china_unicom_dorm_suffix(normalized) {
        return Some(DormitorySsidProfile::ChinaUnicomRouter);
    }
    if has_observed_dorm_suffix(normalized, "bjut-sushe", false)
        || has_observed_dorm_suffix(normalized, "bjutsushe", false)
    {
        return Some(DormitorySsidProfile::ManagedRouter);
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
    .any(|prefix| has_observed_dorm_suffix(normalized, prefix, false))
    .then_some(DormitorySsidProfile::ManagedRouter)
}

pub(crate) fn campus_wifi_kind(ssid: &str) -> Option<CampusWifiKind> {
    let normalized = normalize_ssid(ssid);
    if normalized == "bjut-wifi" {
        return Some(CampusWifiKind::Public);
    }
    dormitory_ssid_profile(&normalized).map(|_| CampusWifiKind::Dormitory)
}

fn normalize_bssid(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', ":")
}

fn bssid_bytes(value: &str) -> Option<[u8; 6]> {
    let normalized = normalize_bssid(value);
    let mut bytes = [0_u8; 6];
    let mut parts = normalized.split(':');
    for byte in &mut bytes {
        let part = parts.next()?;
        if part.len() != 2 {
            return None;
        }
        *byte = u8::from_str_radix(part, 16).ok()?;
    }
    parts.next().is_none().then_some(bytes)
}

fn usable_wifi_bssid(value: &str) -> bool {
    let Some(bytes) = bssid_bytes(value) else {
        return false;
    };
    bytes != [0; 6]
        && bytes != [0xff; 6]
        && bytes != [0x02, 0x00, 0x00, 0x00, 0x00, 0x00]
        && bytes[0] & 1 == 0
}

fn bssid_has_oui(value: &str, observed_ouis: &[[u8; 3]]) -> bool {
    // An OUI is only a scan-derived corroborating signal, not proof of AP
    // ownership: the caller still requires a fresh same-interface identity,
    // campus-local address and a successful protocol probe.
    bssid_bytes(value).is_some_and(|bytes| observed_ouis.iter().any(|oui| bytes[..3] == oui[..]))
}

fn dormitory_bssid_matches_scan(profile: DormitorySsidProfile, bssid: &str) -> bool {
    // GeoPackage captures from the campus roads and dormitory floors contained
    // 1,700+ distinct dormitory BSSIDs. The generated SSID families were tightly
    // clustered by hardware family, so an unfamiliar OUI is treated as a reason
    // to ask for confirmation instead of silently trusting an SSID clone.
    let observed_ouis: &[[u8; 3]] = match profile {
        DormitorySsidProfile::CampusInfrastructure => &[
            [0x5c, 0xc9, 0x99],
            [0xd4, 0x61, 0xfe],
            [0x1c, 0xab, 0x34],
            [0x90, 0xe7, 0x10],
            [0x10, 0x19, 0x65],
            [0x04, 0xd7, 0xa5],
        ],
        DormitorySsidProfile::ManagedRouter => &[
            [0xfa, 0x53, 0x29],
            [0x6c, 0x44, 0x2a],
            [0xfc, 0x73, 0xfb],
            [0x8c, 0x68, 0x3a],
        ],
        DormitorySsidProfile::ChinaUnicomRouter => &[[0xe8, 0x13, 0x6e], [0xf0, 0x9b, 0xb8]],
    };
    bssid_has_oui(bssid, observed_ouis)
}

fn public_bssid_matches_scan(bssid: &str) -> bool {
    // `bjut_wifi` is intentionally open, so its SSID alone is especially easy
    // to clone. These 50 OUIs cover all 1,738 distinct public-network BSSIDs in
    // the supplied road and dormitory scans. New hardware remains usable after
    // an explicit confirmation/whitelist entry instead of being auto-trusted.
    bssid_has_oui(
        bssid,
        &[
            [0x00, 0x08, 0x2f],
            [0x00, 0x23, 0xeb],
            [0x00, 0x25, 0x84],
            [0x00, 0x26, 0xca],
            [0x00, 0x26, 0xcb],
            [0x00, 0x27, 0x0c],
            [0x00, 0x27, 0x0d],
            [0x00, 0x3a, 0x98],
            [0x00, 0x3a, 0x99],
            [0x00, 0x3a, 0x9a],
            [0x04, 0x40, 0xa9],
            [0x04, 0xa9, 0x59],
            [0x04, 0xd7, 0xa5],
            [0x10, 0x19, 0x65],
            [0x14, 0x96, 0x2d],
            [0x1c, 0x1d, 0x86],
            [0x1c, 0x94, 0x68],
            [0x1c, 0xab, 0x34],
            [0x1c, 0xde, 0xa7],
            [0x24, 0x69, 0x68],
            [0x30, 0x5f, 0x77],
            [0x30, 0xf5, 0x27],
            [0x34, 0x62, 0x88],
            [0x34, 0x6b, 0x5b],
            [0x34, 0x6f, 0x90],
            [0x34, 0xdb, 0xfd],
            [0x3c, 0xd2, 0xe5],
            [0x44, 0x1a, 0xfa],
            [0x48, 0xbd, 0x3d],
            [0x5c, 0xc9, 0x99],
            [0x5c, 0xfc, 0x66],
            [0x6c, 0x87, 0x20],
            [0x70, 0x57, 0xbf],
            [0x78, 0x2c, 0x29],
            [0x7c, 0x1e, 0x06],
            [0x84, 0x80, 0x2d],
            [0x88, 0x1d, 0xfc],
            [0x8c, 0x96, 0xa5],
            [0x90, 0xe7, 0x10],
            [0x94, 0x28, 0x2e],
            [0x98, 0xf1, 0x81],
            [0x9c, 0x54, 0xc2],
            [0xa0, 0xec, 0xf9],
            [0xb0, 0x44, 0x14],
            [0xbc, 0x67, 0x1c],
            [0xc0, 0x25, 0x5c],
            [0xd4, 0x61, 0xfe],
            [0xd4, 0x6d, 0x50],
            [0xd4, 0xa2, 0x3d],
            [0xe0, 0xd1, 0x73],
        ],
    )
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
        (10, 17..=27) | (10, 121) | (10, 126) | (10, 226) | (172, 17..=27) | (172, 30)
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
    if transport.eq_ignore_ascii_case("wifi") && !usable_wifi_bssid(bssid) {
        return NetworkTrustResult {
            decision: NetworkTrustDecision::Blocked,
            reason: "当前 Wi-Fi 的 BSSID 缺失或无效，已阻止发送账号密码".to_string(),
            network_key: key,
        };
    }
    if *login_type == LoginType::Type1
        && !transport.eq_ignore_ascii_case("wifi")
        && !transport.eq_ignore_ascii_case("ethernet")
    {
        return NetworkTrustResult {
            decision: NetworkTrustDecision::Blocked,
            reason: "bjut-sushe 协议仅允许在已确认的 Wi-Fi 或有线接口上发送账号密码".to_string(),
            network_key: key,
        };
    }
    if *login_type == LoginType::Type2 && !transport.eq_ignore_ascii_case("wifi") {
        return NetworkTrustResult {
            decision: NetworkTrustDecision::Blocked,
            reason: "bjut_wifi 协议仅允许在已确认的 Wi-Fi 接口上发送账号密码".to_string(),
            network_key: key,
        };
    }
    if *login_type == LoginType::Type3 && !transport.eq_ignore_ascii_case("ethernet") {
        return NetworkTrustResult {
            decision: NetworkTrustDecision::Blocked,
            reason: "lgn 协议仅允许在已确认的有线接口上发送账号密码".to_string(),
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

    let dormitory_profile = dormitory_ssid_profile(&normalized_ssid);
    let allowed = match login_type {
        LoginType::Type1 => {
            transport.eq_ignore_ascii_case("ethernet")
                || dormitory_profile
                    .is_some_and(|profile| dormitory_bssid_matches_scan(profile, bssid))
        }
        LoginType::Type2 => {
            campus_wifi_kind(&normalized_ssid) == Some(CampusWifiKind::Public)
                && public_bssid_matches_scan(bssid)
        }
        LoginType::Type3 => {
            transport.eq_ignore_ascii_case("ethernet")
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
        } else if *login_type == LoginType::Type1 && transport.eq_ignore_ascii_case("ethernet") {
            "bjut-sushe 有线认证要求同一物理接口的校园网地址；当前网络需要明确确认".to_string()
        } else if *login_type == LoginType::Type1 && dormitory_profile.is_some() {
            "宿舍网名称符合扫描特征，但 BSSID 硬件族未经观测；当前网络需要明确确认".to_string()
        } else if *login_type == LoginType::Type2
            && campus_wifi_kind(&normalized_ssid) == Some(CampusWifiKind::Public)
        {
            "公共校园网名称符合扫描特征，但 BSSID 硬件族未经观测；当前网络需要明确确认".to_string()
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
    fn wired_portal_subnet_is_recognized_as_campus_local() {
        assert!(is_campus_local_ip("172.30.201.42"));
        assert!(!is_campus_local_ip("172.31.201.42"));
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
        for china_unicom_variant in [
            "CU_bjut-sushe-28Au",
            "CU_bjut-sushe-28Au_5G",
            "CU_bjut_sushe_8eAs",
            "CU_-bjut-sushe-Q2Jv",
        ] {
            assert_eq!(
                campus_wifi_kind(china_unicom_variant),
                Some(CampusWifiKind::Dormitory),
                "{china_unicom_variant}"
            );
        }
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
            "CU_bjut-sushe-private",
            "CU_bjut-sushe-28Au-guest",
            "CU_bjut-bjut-sushe-sushe-u65B_5G",
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
    fn dormitory_auto_trust_cross_checks_scan_derived_bssid_families() {
        for (ssid, bssid) in [
            ("bjut-sushe-5G-24vF", "fa:53:29:12:34:56"),
            ("bjut_sushe", "5c:c9:99:12:34:56"),
            ("CU_bjut-sushe-28Au", "e8:13:6e:69:e0:48"),
            ("CU_bjut-sushe-6E6z_5G", "f0:9b:b8:0c:c4:d4"),
        ] {
            let result = evaluate_network_trust(NetworkTrustInput {
                login_type: &LoginType::Type1,
                ssid,
                bssid,
                ip: "10.21.2.3",
                transport: "wifi",
                identity_fresh: true,
                whitelist: &[],
                blacklist: &[],
            });
            assert_eq!(result.decision, NetworkTrustDecision::Allowed, "{ssid}");
        }
    }

    #[test]
    fn cloned_dormitory_ssid_with_unknown_oui_requires_confirmation() {
        let result = evaluate_network_trust(NetworkTrustInput {
            login_type: &LoginType::Type1,
            ssid: "CU_bjut-sushe-28Au",
            bssid: "10:20:30:40:50:60",
            ip: "10.21.2.3",
            transport: "wifi",
            identity_fresh: true,
            whitelist: &[],
            blacklist: &[],
        });
        assert_eq!(result.decision, NetworkTrustDecision::NeedsConfirmation);
        assert!(result.reason.contains("BSSID"));
    }

    #[test]
    fn open_public_wifi_also_requires_an_observed_bssid_family() {
        let observed = evaluate_network_trust(NetworkTrustInput {
            login_type: &LoginType::Type2,
            ssid: "bjut_wifi",
            bssid: "44:1a:fa:12:34:56",
            ip: "10.21.2.3",
            transport: "wifi",
            identity_fresh: true,
            whitelist: &[],
            blacklist: &[],
        });
        assert_eq!(observed.decision, NetworkTrustDecision::Allowed);

        let clone = evaluate_network_trust(NetworkTrustInput {
            login_type: &LoginType::Type2,
            ssid: "bjut_wifi",
            bssid: "10:20:30:40:50:60",
            ip: "10.21.2.3",
            transport: "wifi",
            identity_fresh: true,
            whitelist: &[],
            blacklist: &[],
        });
        assert_eq!(clone.decision, NetworkTrustDecision::NeedsConfirmation);
        assert!(clone.reason.contains("BSSID"));
    }

    #[test]
    fn system_placeholder_bssid_is_blocked_even_when_saved_as_trusted() {
        let result = evaluate_network_trust(NetworkTrustInput {
            login_type: &LoginType::Type2,
            ssid: "bjut_wifi",
            bssid: "02:00:00:00:00:00",
            ip: "10.21.2.3",
            transport: "wifi",
            identity_fresh: true,
            whitelist: &["bjut-wifi|02:00:00:00:00:00".to_string()],
            blacklist: &[],
        });
        assert_eq!(result.decision, NetworkTrustDecision::Blocked);
    }

    #[test]
    fn type3_never_sends_credentials_over_wifi_even_when_whitelisted() {
        let result = evaluate_network_trust(NetworkTrustInput {
            login_type: &LoginType::Type3,
            ssid: "bjut_wifi",
            bssid: "44:1a:fa:12:34:56",
            ip: "10.21.2.3",
            transport: "wifi",
            identity_fresh: true,
            whitelist: &["bjut-wifi|44:1a:fa:12:34:56".to_string()],
            blacklist: &[],
        });
        assert_eq!(result.decision, NetworkTrustDecision::Blocked);
    }

    #[test]
    fn type2_never_sends_credentials_over_ethernet_even_when_whitelisted() {
        let result = evaluate_network_trust(NetworkTrustInput {
            login_type: &LoginType::Type2,
            ssid: "bjut_wifi",
            bssid: "44:1a:fa:12:34:56",
            ip: "10.21.2.3",
            transport: "ethernet",
            identity_fresh: true,
            whitelist: &["bjut-wifi|44:1a:fa:12:34:56".to_string()],
            blacklist: &[],
        });
        assert_eq!(result.decision, NetworkTrustDecision::Blocked);
    }

    #[test]
    fn dormitory_type1_accepts_a_same_interface_campus_ethernet_identity() {
        let result = evaluate_network_trust(NetworkTrustInput {
            login_type: &LoginType::Type1,
            ssid: "",
            bssid: "",
            ip: "172.26.33.104",
            transport: "ethernet",
            identity_fresh: true,
            whitelist: &[],
            blacklist: &[],
        });
        assert_eq!(result.decision, NetworkTrustDecision::Allowed);

        let non_campus = evaluate_network_trust(NetworkTrustInput {
            login_type: &LoginType::Type1,
            ssid: "",
            bssid: "",
            ip: "192.168.1.20",
            transport: "ethernet",
            identity_fresh: true,
            whitelist: &[],
            blacklist: &[],
        });
        assert_eq!(non_campus.decision, NetworkTrustDecision::NeedsConfirmation);
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
