use std::collections::HashMap;

use crate::{campus_services, recharge_state};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct RouterBinding {
    pub(crate) router_id: String,
    pub(crate) gateways: Vec<String>,
    pub(crate) ssid: String,
    pub(crate) bssid: String,
    pub(crate) transport: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq)]
pub(crate) struct SavedRouter {
    pub(crate) address: String,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) certificate: Option<String>,
    pub(crate) allow_http: bool,
    #[serde(default)]
    pub(crate) binding: Option<RouterBinding>,
}
impl std::fmt::Debug for SavedRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SavedRouter")
            .field("address", &self.address)
            .field("password", &"[hidden]")
            .finish_non_exhaustive()
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct Account {
    #[serde(alias = "username")]
    pub(crate) user: String,
    #[serde(alias = "password")]
    pub(crate) pass: String,
    #[serde(default, rename = "isDefault", alias = "is_default")]
    pub(crate) is_default: bool,
    #[serde(default, rename = "isDisabled", alias = "is_disabled")]
    pub(crate) is_disabled: Option<bool>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct NetworkProfile {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) ssid: String,
    #[serde(default)]
    pub(crate) bssid: String,
    #[serde(default = "default_profile_login_type")]
    pub(crate) login_type: String,
    #[serde(default)]
    pub(crate) account_order: Vec<String>,
    #[serde(default)]
    pub(crate) auto_login: Option<bool>,
    #[serde(default)]
    pub(crate) auto_login_types: HashMap<String, bool>,
    #[serde(default)]
    pub(crate) check_interval: Option<i32>,
    #[serde(default)]
    pub(crate) check_interval_bg: Option<i32>,
}

/// 用户可覆盖的校园网网关地址。空字符串 = 使用内置值。
/// 只允许 IP 字面量：scheme / 端口 / 路径由协议契约固定，域名绑定证书 SAN。
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct EndpointOverrides {
    #[serde(default, alias = "dormGateway")]
    pub(crate) dorm_gateway: String,
    #[serde(default, alias = "wifiGateway")]
    pub(crate) wifi_gateway: String,
    #[serde(default, alias = "lgnGateway")]
    pub(crate) lgn_gateway: String,
    #[serde(default, alias = "lgn6Gateway")]
    pub(crate) lgn6_gateway: String,
}

impl EndpointOverrides {
    pub(crate) fn is_empty(&self) -> bool {
        self.dorm_gateway.is_empty()
            && self.wifi_gateway.is_empty()
            && self.lgn_gateway.is_empty()
            && self.lgn6_gateway.is_empty()
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct AppConfig {
    #[serde(default)]
    pub(crate) tray: crate::tray_settings::TraySettings,
    #[serde(default)]
    pub(crate) traffic_monitor_enabled: bool,
    // Empty on installations predating explicit opt-in. Normalize once on load.
    #[serde(default)]
    pub(crate) traffic_monitor_scope: String,
    #[serde(default)]
    pub(crate) traffic_retention: crate::usage_history::Retention,
    #[serde(default)]
    pub(crate) prefer_remaining_flow: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) saved_router: Option<SavedRouter>,
    #[serde(default)]
    pub(crate) preferred_interface: String,
    #[serde(default = "default_true")]
    pub(crate) adaptive_network_checks: bool,
    #[serde(default)]
    pub(crate) accounts: Vec<Account>,
    #[serde(default = "default_auto_login", alias = "autoLogin")]
    pub(crate) auto_login: bool,
    #[serde(default = "default_check_interval", alias = "checkInterval")]
    pub(crate) check_interval: i32,
    #[serde(default = "default_check_interval_bg", alias = "checkIntervalBg")]
    pub(crate) check_interval_bg: i32,
    #[serde(default = "default_wifi_change_detect", alias = "wifiChangeDetect")]
    pub(crate) wifi_change_detect: bool,
    #[serde(default = "default_log_level", alias = "logLevel")]
    pub(crate) log_level: String,
    #[serde(default = "default_theme")]
    pub(crate) theme: String,
    #[serde(default = "default_accent_color", alias = "accentColor")]
    pub(crate) accent_color: String,
    #[serde(default = "default_color_mode", alias = "colorMode")]
    pub(crate) color_mode: String,
    #[serde(default, alias = "macosDockVisible")]
    pub(crate) macos_dock_visible: Option<bool>,
    #[serde(default = "default_vpn_compatibility", alias = "vpnCompatibility")]
    pub(crate) vpn_compatibility: String,
    #[serde(default, alias = "vpnMaximumUntil")]
    pub(crate) vpn_maximum_until: Option<i64>,
    /// 网关地址覆盖。缺省/全空 = 完全使用内置值，行为与之前逐字节一致。
    #[serde(
        default,
        alias = "endpointOverrides",
        skip_serializing_if = "EndpointOverrides::is_empty"
    )]
    pub(crate) endpoint_overrides: EndpointOverrides,
    /// 宿舍网关证书指纹覆盖（64 位十六进制，可带冒号）。空 = 使用内置指纹。
    /// 只能替换指纹，不能关闭 pin 例外。
    #[serde(
        default,
        alias = "dormCertSha256",
        skip_serializing_if = "String::is_empty"
    )]
    pub(crate) dorm_cert_sha256: String,
    #[serde(default)]
    pub(crate) whitelist: Vec<String>,
    #[serde(default)]
    pub(crate) blacklist: Vec<String>,
    #[serde(default)]
    pub(crate) network_profiles: Vec<NetworkProfile>,
    #[serde(default = "default_usage_alerts")]
    pub(crate) usage_alerts: bool,
    #[serde(default = "default_balance_alert_threshold")]
    pub(crate) balance_alert_threshold: f64,
    #[serde(default = "default_flow_alert_threshold")]
    pub(crate) flow_alert_threshold: f64,
    #[serde(
        default = "default_android_notification_mode",
        alias = "androidNotificationMode"
    )]
    pub(crate) android_notification_mode: String,
    #[serde(default = "default_true", alias = "androidNotifyNetworkStatus")]
    pub(crate) android_notify_network_status: bool,
    #[serde(default = "default_true", alias = "androidNotifyLoginResults")]
    pub(crate) android_notify_login_results: bool,
    #[serde(default = "default_true", alias = "androidNotifyBackgroundErrors")]
    pub(crate) android_notify_background_errors: bool,
    #[serde(
        default,
        rename = "campusServiceSessions",
        alias = "campus_service_sessions"
    )]
    pub(crate) campus_service_sessions: Vec<campus_services::PersistedCampusSession>,
    #[serde(
        default,
        rename = "rechargeTransactions",
        alias = "recharge_transactions"
    )]
    pub(crate) recharge_transactions: recharge_state::RechargeJournal,
}

pub(crate) fn default_auto_login() -> bool {
    false
}

pub(crate) fn default_check_interval() -> i32 {
    15
}

pub(crate) fn default_check_interval_bg() -> i32 {
    60
}

pub(crate) fn default_wifi_change_detect() -> bool {
    true
}

pub(crate) fn default_log_level() -> String {
    "info".to_string()
}

pub(crate) fn default_theme() -> String {
    "basic".to_string()
}

pub(crate) fn default_accent_color() -> String {
    "blue".to_string()
}

pub(crate) fn default_color_mode() -> String {
    "system".to_string()
}

pub(crate) fn default_vpn_compatibility() -> String {
    "high".to_string()
}

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) fn default_profile_login_type() -> String {
    "auto".to_string()
}

pub(crate) fn default_usage_alerts() -> bool {
    true
}

pub(crate) fn default_balance_alert_threshold() -> f64 {
    10.0
}

pub(crate) fn default_flow_alert_threshold() -> f64 {
    5.0
}

pub(crate) fn default_android_notification_mode() -> String {
    "combined".to_string()
}

/// Earlier builds enabled collection by default. A missing scope requires a
/// fresh explicit opt-in, rather than treating that old default as consent.
pub(crate) fn normalize_traffic_monitor(config: &mut AppConfig) {
    config.traffic_retention.normalize();
    if !matches!(config.traffic_monitor_scope.as_str(), "local" | "all") {
        config.traffic_monitor_enabled = false;
        config.traffic_monitor_scope = "local".into();
    }
}

/// Validate the user-editable network endpoints before a config is persisted
/// or imported. The same pure function backs both paths so they cannot drift.
pub(crate) fn validate_endpoint_config(config: &AppConfig) -> Result<(), String> {
    crate::portal_auth::endpoints::validate_endpoint_overrides(&config.endpoint_overrides)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_config() -> AppConfig {
        // Only fields that existed before this change; the two new fields must
        // fall back to their defaults.
        serde_json::from_value(serde_json::json!({
            "autoLogin": false,
            "checkInterval": 30,
            "checkIntervalBg": 90,
            "wifiChangeDetect": true,
            "vpnCompatibility": "high"
        }))
        .expect("legacy config must still deserialize")
    }

    #[test]
    fn legacy_configuration_without_overrides_still_loads() {
        let config = legacy_config();
        assert!(config.endpoint_overrides.is_empty());
        assert!(config.dorm_cert_sha256.is_empty());
        assert_eq!(
            crate::portal_auth::PortalSettings::from_config(&config),
            crate::portal_auth::PortalSettings::default()
        );
    }

    #[test]
    fn endpoint_overrides_round_trip_through_serde() {
        let overrides = EndpointOverrides {
            dorm_gateway: "10.0.0.9".into(),
            wifi_gateway: "10.0.0.10".into(),
            lgn_gateway: "10.0.0.11".into(),
            lgn6_gateway: "2001:db8::9".into(),
        };
        let encoded = serde_json::to_value(&overrides).unwrap();
        let decoded: EndpointOverrides = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, overrides);
    }

    #[test]
    fn app_config_keeps_serialized_shape_when_overrides_are_unset() {
        let config = legacy_config();
        let encoded = serde_json::to_value(&config).unwrap();
        let object = encoded.as_object().unwrap();
        assert!(!object.contains_key("endpoint_overrides"));
        assert!(!object.contains_key("dorm_cert_sha256"));
    }

    #[test]
    fn app_config_accepts_camel_case_override_aliases() {
        let config: AppConfig = serde_json::from_value(serde_json::json!({
            "endpointOverrides": { "dormGateway": "10.0.0.9" },
            "dormCertSha256": "aa"
        }))
        .unwrap();
        assert_eq!(config.endpoint_overrides.dorm_gateway, "10.0.0.9");
        assert_eq!(config.dorm_cert_sha256, "aa");
    }
}
