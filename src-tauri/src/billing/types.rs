use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscoveredCampusAccount {
    pub user: String,
    pub pass: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BillingSnapshot {
    pub account: String,
    pub balance: String,
    pub remaining_flow: String,
    pub used_flow: Option<String>,
    pub status: Option<String>,
    pub status_reason: Option<String>,
    pub package: Option<String>,
    pub package_detail: Option<String>,
    pub billing_cycle: Option<String>,
    pub updated_at: String,
    pub login_history: Vec<BillingLoginRecord>,
    pub online_sessions: Vec<BillingOnlineSession>,
    pub offline_tip: Option<String>,
    pub mauth_enabled: Option<bool>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BillingLoginRecord {
    pub login_at: String,
    pub logout_at: String,
    pub ip: String,
    pub ipv6: String,
    pub mac: String,
    pub duration_minutes: String,
    pub used_flow_mb: String,
    pub billing_mode: String,
    pub amount: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BillingOnlineSession {
    pub login_at: String,
    pub ip: String,
    pub ipv6: String,
    pub mac: String,
    pub duration_minutes: String,
    pub used_flow_mb: String,
    pub session_id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BillingTable {
    pub total: u64,
    pub rows: Vec<BTreeMap<String, String>>,
    pub summary: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BillingPackageOption {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BillingPasswordPolicy {
    pub min_length: usize,
    pub max_length: usize,
    pub require_uppercase: bool,
    pub require_lowercase: bool,
    pub require_digit: bool,
    pub require_special: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BillingSecurityQuestion {
    pub id: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BillingServiceState {
    pub account_status: Option<String>,
    pub status_reason: Option<String>,
    pub current_package_id: Option<String>,
    pub current_package: Option<String>,
    pub package_detail: Option<String>,
    pub next_settlement_date: Option<String>,
    pub can_stop_now: bool,
    pub can_reopen_now: bool,
    pub package_scheduled: bool,
    pub scheduled_package_id: Option<String>,
    pub scheduled_package: Option<String>,
    pub consume_limit: Option<String>,
    pub current_cycle_spend: Option<String>,
    pub balance: Option<String>,
    pub package_options: Vec<BillingPackageOption>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BillingCenterData {
    pub account: String,
    pub overview: BillingSnapshot,
    pub fetched_at: String,
    pub query_start_date: String,
    pub query_end_date: String,
    pub query_year: String,
    pub usage_records: BillingTable,
    pub monthly_bills: BillingTable,
    pub payments: BillingTable,
    pub operations: BillingTable,
    pub stop_logs: BillingTable,
    pub reopen_logs: BillingTable,
    pub package_logs: BillingTable,
    pub devices: BillingTable,
    pub tariff_groups: BillingTable,
    pub service: BillingServiceState,
    pub password_policy: BillingPasswordPolicy,
    pub security_questions: Vec<BillingSecurityQuestion>,
    pub recharge_available: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BillingQuestionAnswer {
    pub question_id: String,
    pub answer: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BillingActionRequest {
    pub action: String,
    pub package_id: Option<String>,
    pub consume_limit: Option<String>,
    pub mac: Option<String>,
    pub old_password: Option<String>,
    pub new_password: Option<String>,
    #[serde(default)]
    pub questions: Vec<BillingQuestionAnswer>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BillingActionResult {
    pub message: String,
    pub password_changed: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BillingRecordQuery {
    pub kind: String,
    pub page: u32,
    pub page_size: u32,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub year: Option<String>,
    #[serde(default)]
    pub all: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BillingRecordResult {
    pub kind: String,
    pub page: u32,
    pub page_size: u32,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub year: Option<String>,
    pub all: bool,
    pub table: BillingTable,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BillingError {
    Network(String),
    Protocol(String),
    InvalidRequest(String),
    ActionRejected(String),
    CaptchaRequired,
    AuthenticationRejected,
}

impl BillingError {
    pub(crate) fn user_message(&self) -> String {
        match self {
            Self::Network(detail) => format!("计费系统暂不可达：{detail}"),
            Self::Protocol(detail) => format!("计费系统响应格式异常：{detail}"),
            Self::InvalidRequest(detail) => format!("计费请求未提交：{detail}"),
            Self::ActionRejected(detail) => format!("计费系统未执行操作：{detail}"),
            Self::CaptchaRequired => {
                "计费系统当前要求图形验证码；本次未提交账号密码，请稍后重试或先在浏览器完成验证"
                    .to_string()
            }
            Self::AuthenticationRejected => {
                "计费系统拒绝登录；为避免触发验证码，本次没有自动重试".to_string()
            }
        }
    }
}
