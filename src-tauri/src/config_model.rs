use std::collections::HashMap;

use crate::{campus_services, recharge_state};

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
