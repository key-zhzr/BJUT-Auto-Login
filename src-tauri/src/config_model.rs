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
