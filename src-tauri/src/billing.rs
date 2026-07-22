use super::{query_campus_dns_ipv4, redact_request_error, VpnCompatibility, LGN_HOST};
use chrono::Datelike;
use futures_util::{stream::FuturesUnordered, StreamExt};
use reqwest::header::{
    HeaderMap, ACCEPT, ACCEPT_LANGUAGE, COOKIE, LOCATION, ORIGIN, REFERER, SET_COOKIE,
};
use reqwest::{Client, Response, Url};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const BILLING_HOST: &str = "jfself.bjut.edu.cn";
const BILLING_FIXED_ADDRESS: Ipv4Addr = Ipv4Addr::new(172, 21, 0, 16);
const BILLING_ORIGIN: &str = "https://jfself.bjut.edu.cn";
const BILLING_LOGIN_URL: &str = "https://jfself.bjut.edu.cn/Self/login/?302=LI";
const BILLING_LOGOUT_URL: &str = "https://jfself.bjut.edu.cn/Self/login/logout";
const BILLING_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";
const ACCOUNT_DISCOVERY_PATH: &str = "/eportal/portal/self";
const ACCOUNT_DISCOVERY_PORT: u16 = 802;
const BILLING_EPORTAL_LOGIN_PATH: &str = "/Self/login/eportalLogin";
const BILLING_DASHBOARD_PATH: &str = "/Self/dashboard";
const MAX_REDIRECTS: usize = 5;
const MAX_FULL_EXPORT_ROWS: u64 = 50_000;

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

#[derive(Clone, Debug, Default, PartialEq)]
struct ActivePackageReservation {
    current_package: Option<String>,
    scheduled_package: String,
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

#[derive(Clone, Default)]
struct SessionCookies {
    values: BTreeMap<String, String>,
}

impl SessionCookies {
    fn absorb(&mut self, headers: &HeaderMap) {
        for value in headers.get_all(SET_COOKIE).iter() {
            let Ok(raw) = value.to_str() else { continue };
            let Some(pair) = raw.split(';').next() else {
                continue;
            };
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            let name = name.trim();
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
            {
                continue;
            }
            if value.is_empty() {
                self.values.remove(name);
            } else {
                self.values
                    .insert(name.to_string(), value.trim().to_string());
            }
        }
    }

    fn header(&self) -> Option<String> {
        if self.values.is_empty() {
            None
        } else {
            Some(
                self.values
                    .iter()
                    .map(|(name, value)| format!("{name}={value}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        }
    }

    fn merge(&mut self, other: Self) {
        self.values.extend(other.values);
    }
}

#[derive(Clone, Default)]
struct HostCookies {
    values: BTreeMap<String, SessionCookies>,
}

impl HostCookies {
    fn absorb(&mut self, url: &Url, headers: &HeaderMap) {
        if let Some(host) = url.host_str() {
            self.values
                .entry(host.to_ascii_lowercase())
                .or_default()
                .absorb(headers);
        }
    }

    fn header(&self, url: &Url) -> Option<String> {
        self.values
            .get(&url.host_str()?.to_ascii_lowercase())
            .and_then(SessionCookies::header)
    }
}

#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DashboardUser {
    left_money: Option<f64>,
    installment_flag: Option<f64>,
    use_money: Option<f64>,
    use_flag: Option<i64>,
    stop_reason: Option<String>,
    service_default: Option<DashboardService>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DashboardService {
    id: Option<i64>,
    default_name: Option<String>,
    extend: Option<String>,
}

#[derive(Clone)]
struct BillingSession {
    client: Client,
    cookies: SessionCookies,
    dashboard_url: Url,
    dashboard_html: String,
    ajax_csrf_token: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BillingRecordKind {
    Usage,
    Monthly,
    Payments,
    Operations,
    StopLogs,
    ReopenLogs,
    PackageLogs,
}

struct BillingRecordSpec {
    name: &'static str,
    label: &'static str,
    path: &'static str,
    referer_path: &'static str,
    array_fields: &'static [&'static str],
    sort_name: Option<&'static str>,
    sort_order: &'static str,
    date_filter: bool,
    year_filter: bool,
}

const USAGE_RECORD_FIELDS: &[&str] = &[
    "loginTime",
    "logoutTime",
    "time",
    "flow",
    "costMoney",
    "internetUpFlow",
    "internetDownFlow",
    "chinanetUpFlow",
    "chinanetDownFlow",
    "userIp",
    "userIp1",
    "macAddress",
    "nasIp",
    "nasPort",
];
const STOP_LOG_FIELDS: &[&str] = &["fldoperatedate", "fldoperateid", "fldadminid", "fldmemo"];
const REOPEN_LOG_FIELDS: &[&str] = &[
    "fldoperatedate",
    "fldoperateid",
    "fldnewvalue",
    "fldadminid",
    "fldmemo",
];
const PACKAGE_LOG_FIELDS: &[&str] = &[
    "fldchangedate",
    "fldexcutedate",
    "flddefaultname1",
    "flddefaultname2",
    "fldstate",
    "fldstatedate",
    "fldextend",
];
#[derive(Clone, Debug)]
struct ValidatedBillingRecordQuery {
    kind: BillingRecordKind,
    page: u32,
    page_size: u32,
    start_date: Option<String>,
    end_date: Option<String>,
    year: Option<String>,
    all: bool,
}

pub(crate) async fn fetch_center<F>(
    account: &str,
    password: &str,
    compatibility: VpnCompatibility,
    progress: F,
) -> Result<BillingCenterData, BillingError>
where
    F: Fn(&str, u8) + Send + Sync + 'static,
{
    progress("正在连接并登录计费系统", 5);
    let mut session = authenticate(account, password, compatibility).await?;
    let result = async {
        progress("登录成功，正在解析账户概览", 18);
        let mut overview = parse_dashboard_bounded(&session.dashboard_html, account).await?;
        populate_dashboard_details(&mut session, &mut overview, &progress).await;
        let end_date = chrono::Local::now().date_naive();
        let start_date = end_date - chrono::Duration::days(60);
        let query_start_date = start_date.format("%Y-%m-%d").to_string();
        let query_end_date = end_date.format("%Y-%m-%d").to_string();
        let query_year = end_date.format("%Y").to_string();
        let mut warnings = Vec::new();
        // The seven potentially large bill and operation tables are queried
        // only after the user presses "查询". A center refresh reads the
        // compact service pages needed to render controls and status.
        progress("正在读取账号服务", 68);
        let stop_html = get_page_bounded(
            &mut session,
            "/Self/service/goStop",
            "报停页面",
            &mut warnings,
        )
        .await;
        let reopen_html = get_page_bounded(
            &mut session,
            "/Self/service/goReopen",
            "复通页面",
            &mut warnings,
        )
        .await;
        let package_html = get_page_bounded(
            &mut session,
            "/Self/service/package",
            "预约套餐页面",
            &mut warnings,
        )
        .await;
        let active_package_reservation =
            fetch_active_package_reservation_bounded(&mut session, &mut warnings).await;
        let protect_html = get_page_bounded(
            &mut session,
            "/Self/service/consumeProtect",
            "消费保护页面",
            &mut warnings,
        )
        .await;

        let device_params = [
            ("pageSize", "100"),
            ("pageNumber", "1"),
            ("searchText", ""),
            ("sortName", "2"),
            ("sortOrder", "DESC"),
        ];
        progress("正在读取已绑定设备", 82);
        let (_, devices) = fetch_page_table_bounded(
            &mut session,
            "/Self/service/myMac",
            "/Self/service/getMacList",
            &device_params,
            &["online", "mac", "device", "lastLoginAt", "lastIp"],
            "设备列表",
            &mut warnings,
        )
        .await;

        progress("正在读取安全设置", 91);
        let questions_html = get_page_bounded(
            &mut session,
            "/Self/setting/passwordQuestion",
            "密码保护页面",
            &mut warnings,
        )
        .await;

        let data = BillingCenterData {
            account: account.to_string(),
            overview,
            fetched_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            query_start_date,
            query_end_date,
            query_year,
            usage_records: BillingTable::default(),
            monthly_bills: BillingTable::default(),
            payments: BillingTable::default(),
            operations: BillingTable::default(),
            stop_logs: BillingTable::default(),
            reopen_logs: BillingTable::default(),
            package_logs: BillingTable::default(),
            devices,
            tariff_groups: BillingTable::default(),
            service: parse_service_state(
                &session.dashboard_html,
                &stop_html,
                &reopen_html,
                &package_html,
                &protect_html,
                active_package_reservation.as_ref(),
            ),
            // Password changes are handled by BJUT unified authentication,
            // whose currently deployed rule was verified from /api/register/rules.
            password_policy: BillingPasswordPolicy {
                min_length: 12,
                max_length: 16,
                require_uppercase: true,
                require_lowercase: true,
                require_digit: true,
                require_special: true,
            },
            security_questions: parse_security_questions(&questions_html),
            // Recharge is provided through unified authentication and the
            // mobile portal, not the legacy jfself recharge page.
            recharge_available: true,
            warnings,
        };
        Ok(data)
    }
    .await;
    logout(&mut session).await;
    if result.is_ok() {
        progress("计费中心数据读取完成", 100);
    }
    result
}

pub(crate) async fn query_records(
    account: &str,
    password: &str,
    compatibility: VpnCompatibility,
    request: &BillingRecordQuery,
) -> Result<BillingRecordResult, BillingError> {
    let query = validate_record_query(request)?;
    let spec = record_spec(query.kind);
    let mut session = authenticate(account, password, compatibility).await?;
    let result = async {
        // Load the corresponding module before its AJAX endpoint so the
        // session has the page-local CSRF state expected by jfself.
        let _ = get_page_text(&mut session, spec.referer_path).await?;
        let table = if query.all {
            fetch_all_record_pages(&mut session, &query, &spec).await?
        } else {
            fetch_record_page(&mut session, &query, &spec, query.page, query.page_size).await?
        };
        Ok(BillingRecordResult {
            kind: spec.name.to_string(),
            page: if query.all { 1 } else { query.page },
            page_size: if query.all { 100 } else { query.page_size },
            start_date: query.start_date.clone(),
            end_date: query.end_date.clone(),
            year: query.year.clone(),
            all: query.all,
            table,
        })
    }
    .await;
    logout(&mut session).await;
    result
}

fn validate_record_query(
    request: &BillingRecordQuery,
) -> Result<ValidatedBillingRecordQuery, BillingError> {
    let kind = match request.kind.as_str() {
        "usage" => BillingRecordKind::Usage,
        "monthly" => BillingRecordKind::Monthly,
        "payments" => BillingRecordKind::Payments,
        "operations" => BillingRecordKind::Operations,
        "stopLogs" => BillingRecordKind::StopLogs,
        "reopenLogs" => BillingRecordKind::ReopenLogs,
        "packageLogs" => BillingRecordKind::PackageLogs,
        _ => {
            return Err(BillingError::InvalidRequest(
                "不支持的计费记录类型".to_string(),
            ));
        }
    };
    if request.page == 0 || request.page > 100_000 {
        return Err(BillingError::InvalidRequest(
            "记录页码超出允许范围".to_string(),
        ));
    }
    if !matches!(request.page_size, 10 | 20 | 25 | 50 | 100) {
        return Err(BillingError::InvalidRequest(
            "每页记录数只支持 10、20、25、50 或 100".to_string(),
        ));
    }

    let spec = record_spec(kind);
    let today = chrono::Local::now().date_naive();
    let (start_date, end_date) = if spec.date_filter {
        let start_raw = request
            .start_date
            .as_deref()
            .ok_or_else(|| BillingError::InvalidRequest("请选择查询开始日期".to_string()))?;
        let end_raw = request
            .end_date
            .as_deref()
            .ok_or_else(|| BillingError::InvalidRequest("请选择查询结束日期".to_string()))?;
        let start = chrono::NaiveDate::parse_from_str(start_raw, "%Y-%m-%d")
            .map_err(|_| BillingError::InvalidRequest("查询开始日期格式无效".to_string()))?;
        let end = chrono::NaiveDate::parse_from_str(end_raw, "%Y-%m-%d")
            .map_err(|_| BillingError::InvalidRequest("查询结束日期格式无效".to_string()))?;
        if start > end || end > today || (end - start).num_days() > 60 {
            return Err(BillingError::InvalidRequest(
                "查询日期必须截至今天，且范围不能超过 60 天".to_string(),
            ));
        }
        (Some(start_raw.to_string()), Some(end_raw.to_string()))
    } else {
        (None, None)
    };

    let year = if spec.year_filter {
        let raw = request
            .year
            .as_deref()
            .ok_or_else(|| BillingError::InvalidRequest("请选择历史账单年份".to_string()))?;
        let parsed = raw
            .parse::<i32>()
            .map_err(|_| BillingError::InvalidRequest("历史账单年份格式无效".to_string()))?;
        if raw.len() != 4 || !(2000..=today.year()).contains(&parsed) {
            return Err(BillingError::InvalidRequest(
                "历史账单年份超出允许范围".to_string(),
            ));
        }
        Some(raw.to_string())
    } else {
        None
    };

    Ok(ValidatedBillingRecordQuery {
        kind,
        page: request.page,
        page_size: request.page_size,
        start_date,
        end_date,
        year,
        all: request.all,
    })
}

fn record_spec(kind: BillingRecordKind) -> BillingRecordSpec {
    match kind {
        BillingRecordKind::Usage => BillingRecordSpec {
            name: "usage",
            label: "上网记录",
            path: "/Self/bill/getUserOnlineLog",
            referer_path: "/Self/bill/userOnlineLog",
            array_fields: USAGE_RECORD_FIELDS,
            sort_name: Some("loginTime"),
            sort_order: "DESC",
            date_filter: true,
            year_filter: false,
        },
        BillingRecordKind::Monthly => BillingRecordSpec {
            name: "monthly",
            label: "历史账单",
            path: "/Self/bill/getMonthPay",
            referer_path: "/Self/bill/monthPay",
            array_fields: &[
                "startAt",
                "endAt",
                "package",
                "baseFee",
                "usageFee",
                "durationMinutes",
                "flowMb",
                "billedAt",
            ],
            sort_name: Some("0"),
            sort_order: "DESC",
            date_filter: false,
            year_filter: true,
        },
        BillingRecordKind::Payments => BillingRecordSpec {
            name: "payments",
            label: "充值明细",
            path: "/Self/bill/getPayMent",
            referer_path: "/Self/bill/payMent",
            array_fields: &["paidAt", "type", "amount", "terminal", "note"],
            sort_name: Some("0"),
            sort_order: "DESC",
            date_filter: true,
            year_filter: false,
        },
        BillingRecordKind::Operations => BillingRecordSpec {
            name: "operations",
            label: "业务办理记录",
            path: "/Self/bill/getOperatorLog",
            referer_path: "/Self/bill/operatorLog",
            array_fields: &["operatedAt", "description", "terminal", "unused", "note"],
            sort_name: Some("0"),
            sort_order: "DESC",
            date_filter: true,
            year_filter: false,
        },
        BillingRecordKind::StopLogs => BillingRecordSpec {
            name: "stopLogs",
            label: "报停记录",
            path: "/Self/service/getStopLog",
            referer_path: "/Self/service/goStop",
            array_fields: STOP_LOG_FIELDS,
            sort_name: None,
            sort_order: "DESC",
            date_filter: false,
            year_filter: false,
        },
        BillingRecordKind::ReopenLogs => BillingRecordSpec {
            name: "reopenLogs",
            label: "复通记录",
            path: "/Self/service/goReopenLog",
            referer_path: "/Self/service/goReopen",
            array_fields: REOPEN_LOG_FIELDS,
            sort_name: None,
            sort_order: "DESC",
            date_filter: false,
            year_filter: false,
        },
        BillingRecordKind::PackageLogs => BillingRecordSpec {
            name: "packageLogs",
            label: "预约套餐记录",
            path: "/Self/service/packageLog",
            referer_path: "/Self/service/package",
            array_fields: PACKAGE_LOG_FIELDS,
            sort_name: Some("fldchangedate"),
            sort_order: "DESC",
            date_filter: false,
            year_filter: false,
        },
    }
}

async fn fetch_record_page(
    session: &mut BillingSession,
    query: &ValidatedBillingRecordQuery,
    spec: &BillingRecordSpec,
    page: u32,
    page_size: u32,
) -> Result<BillingTable, BillingError> {
    let mut params = vec![
        ("pageSize".to_string(), page_size.to_string()),
        ("pageNumber".to_string(), page.to_string()),
        ("searchText".to_string(), String::new()),
    ];
    if let Some(sort_name) = spec.sort_name {
        params.push(("sortName".to_string(), sort_name.to_string()));
    }
    params.push(("sortOrder".to_string(), spec.sort_order.to_string()));
    if let (Some(start), Some(end)) = (&query.start_date, &query.end_date) {
        params.push(("startTime".to_string(), start.clone()));
        params.push(("endTime".to_string(), end.clone()));
    }
    if let Some(year) = &query.year {
        params.push(("year".to_string(), year.clone()));
    }
    get_ajax_text_owned(session, spec.path, &params, spec.referer_path)
        .await
        .and_then(|text| parse_billing_table(&text, spec.array_fields, spec.label))
}

async fn fetch_all_record_pages(
    session: &mut BillingSession,
    query: &ValidatedBillingRecordQuery,
    spec: &BillingRecordSpec,
) -> Result<BillingTable, BillingError> {
    let mut combined = fetch_record_page(session, query, spec, 1, 100).await?;
    if combined.total > MAX_FULL_EXPORT_ROWS {
        return Err(BillingError::InvalidRequest(format!(
            "{}共有 {} 条，超过单次完整导出的安全上限；请缩小日期范围",
            spec.label, combined.total
        )));
    }
    let expected_total = combined.total;
    let page_count = expected_total.div_ceil(100);
    for page in 2..=page_count {
        let next = fetch_record_page(session, query, spec, page as u32, 100).await?;
        if next.rows.is_empty() && combined.rows.len() < expected_total as usize {
            return Err(BillingError::Protocol(format!(
                "{}在完整导出过程中提前结束，请稍后重试",
                spec.label
            )));
        }
        combined.rows.extend(next.rows);
        if combined.rows.len() >= expected_total as usize {
            break;
        }
    }
    combined.rows.truncate(expected_total as usize);
    Ok(combined)
}

pub(crate) async fn perform_action(
    account: &str,
    password: &str,
    compatibility: VpnCompatibility,
    request: &BillingActionRequest,
) -> Result<BillingActionResult, BillingError> {
    if !matches!(
        request.action.as_str(),
        "stopNow"
            | "reopenNow"
            | "schedulePackage"
            | "cancelPackage"
            | "setConsumeLimit"
            | "bindMac"
            | "unbindMac"
            | "changePassword"
            | "updateQuestions"
    ) {
        return Err(BillingError::InvalidRequest("不支持的计费操作".to_string()));
    }

    let mut session = authenticate(account, password, compatibility).await?;
    let result = async {
        match request.action.as_str() {
            "stopNow" => stop_account(&mut session).await,
            "reopenNow" => reopen_account(&mut session).await,
            "schedulePackage" => {
                schedule_package(&mut session, request.package_id.as_deref()).await
            }
            "cancelPackage" => cancel_package(&mut session).await,
            "setConsumeLimit" => {
                set_consume_limit(&mut session, request.consume_limit.as_deref()).await
            }
            "bindMac" => bind_mac(&mut session, request.mac.as_deref()).await,
            "unbindMac" => unbind_mac(&mut session, request.mac.as_deref()).await,
            "changePassword" => {
                change_password(
                    &mut session,
                    password,
                    request.old_password.as_deref(),
                    request.new_password.as_deref(),
                )
                .await
            }
            "updateQuestions" => {
                update_security_questions(
                    &mut session,
                    password,
                    request.old_password.as_deref(),
                    &request.questions,
                )
                .await
            }
            _ => unreachable!(),
        }
    }
    .await;
    logout(&mut session).await;
    result
}

async fn stop_account(session: &mut BillingSession) -> Result<BillingActionResult, BillingError> {
    let user = dashboard_user(&session.dashboard_html);
    if user.use_flag != Some(1) {
        return Err(BillingError::ActionRejected(
            "账号当前不是正常状态，无法办理报停".to_string(),
        ));
    }
    let page = get_page_text(session, "/Self/service/goStop").await?;
    if !has_enabled_element_id(&page, "stopNow") {
        return Err(BillingError::ActionRejected(
            "计费系统当前未向该账号开放所选报停方式".to_string(),
        ));
    }
    let text = post_form_action(
        session,
        "/Self/service/stop",
        "/Self/service/goStop",
        vec![("flag".to_string(), "1".to_string())],
        true,
    )
    .await?;
    Ok(BillingActionResult {
        message: parse_action_response(&text, "账号已报停")?,
        password_changed: false,
    })
}

async fn reopen_account(session: &mut BillingSession) -> Result<BillingActionResult, BillingError> {
    let user = dashboard_user(&session.dashboard_html);
    if user.use_flag == Some(1) && user.stop_reason.is_none() {
        return Err(BillingError::ActionRejected(
            "账号当前已经是正常状态，无需复通".to_string(),
        ));
    }
    let page = get_page_text(session, "/Self/service/goReopen").await?;
    if !has_enabled_element_id(&page, "reOpenNow") {
        return Err(BillingError::ActionRejected(
            "计费系统当前未向该账号开放所选复通方式".to_string(),
        ));
    }
    let text = post_form_action(
        session,
        "/Self/service/reOpen",
        "/Self/service/goReopen",
        vec![("flag".to_string(), "1".to_string())],
        true,
    )
    .await?;
    Ok(BillingActionResult {
        message: parse_action_response(&text, "账号已复通")?,
        password_changed: false,
    })
}

async fn schedule_package(
    session: &mut BillingSession,
    package_id: Option<&str>,
) -> Result<BillingActionResult, BillingError> {
    let package_id = package_id
        .filter(|value| !value.is_empty() && value.len() <= 32)
        .ok_or_else(|| BillingError::InvalidRequest("请选择要预约的套餐".to_string()))?;
    let page = get_page_text(session, "/Self/service/package").await?;
    let current_package_id = dashboard_user(&session.dashboard_html)
        .service_default
        .and_then(|service| service.id)
        .map(|id| id.to_string());
    if current_package_id.as_deref() == Some(package_id) {
        let reservation = package_reservation_for_action(session, &page).await?;
        return cancel_package_from_page(session, &page, reservation.as_ref()).await;
    }
    if !parse_package_options(&page)
        .iter()
        .any(|option| option.id == package_id)
    {
        return Err(BillingError::ActionRejected(
            "套餐列表已经变化，请刷新后重新选择".to_string(),
        ));
    }
    let csrf = action_csrf_token(&page, "doPackage")?;
    let text = post_form_action(
        session,
        "/Self/service/doPackage",
        "/Self/service/package",
        vec![
            ("csrftoken".to_string(), csrf),
            ("serid".to_string(), package_id.to_string()),
        ],
        false,
    )
    .await?;
    Ok(BillingActionResult {
        message: parse_action_response(&text, "套餐预约已提交")?,
        password_changed: false,
    })
}

async fn cancel_package(session: &mut BillingSession) -> Result<BillingActionResult, BillingError> {
    let page = get_page_text(session, "/Self/service/package").await?;
    let reservation = package_reservation_for_action(session, &page).await?;
    cancel_package_from_page(session, &page, reservation.as_ref()).await
}

async fn package_reservation_for_action(
    session: &mut BillingSession,
    page: &str,
) -> Result<Option<ActivePackageReservation>, BillingError> {
    match fetch_active_package_reservation(session).await {
        Ok(reservation) => Ok(reservation),
        Err(_) if has_package_cancel_control(page) => Ok(None),
        Err(error) => Err(error),
    }
}

async fn cancel_package_from_page(
    session: &mut BillingSession,
    page: &str,
    reservation: Option<&ActivePackageReservation>,
) -> Result<BillingActionResult, BillingError> {
    if reservation.is_none() && !has_package_cancel_control(page) {
        return Err(BillingError::ActionRejected(
            "当前没有可取消的套餐预约".to_string(),
        ));
    }
    let current_package = dashboard_user(&session.dashboard_html)
        .service_default
        .and_then(|service| service.default_name);
    let scheduled_package = reservation
        .map(|value| value.scheduled_package.clone())
        .or_else(|| scheduled_package_name(page));
    if scheduled_package.is_some()
        && !package_reservation_is_distinct(
            current_package.as_deref(),
            scheduled_package.as_deref(),
        )
    {
        return Err(BillingError::ActionRejected(
            "下一周期将继续使用当前套餐，没有可取消的套餐预约".to_string(),
        ));
    }
    let text = post_form_action(
        session,
        "/Self/service/undoPackage",
        "/Self/service/package",
        Vec::new(),
        true,
    )
    .await?;
    Ok(BillingActionResult {
        message: parse_action_response(&text, "套餐预约已取消")?,
        password_changed: false,
    })
}

async fn set_consume_limit(
    session: &mut BillingSession,
    requested: Option<&str>,
) -> Result<BillingActionResult, BillingError> {
    let requested = validate_consume_limit(requested)?;
    let page = get_page_text(session, "/Self/service/consumeProtect").await?;
    let page_user = dashboard_user(&page);
    let numeric = requested.parse::<f64>().map_err(|_| {
        BillingError::InvalidRequest("消费限额必须是非负数字，最多三位小数".to_string())
    })?;
    if numeric != 999_999.0 && page_user.use_money.is_some_and(|spent| numeric < spent) {
        return Err(BillingError::InvalidRequest(
            "消费限额不能低于本周期已经产生的消费".to_string(),
        ));
    }
    let csrf = form_csrf_token(&page)?;
    let response = post_form_action(
        session,
        "/Self/service/changeConsumeProtect",
        "/Self/service/consumeProtect",
        vec![
            ("csrftoken".to_string(), csrf),
            ("consumeLimit".to_string(), requested.clone()),
        ],
        false,
    )
    .await?;
    if let Ok(message) = parse_action_response(&response, "消费保护限额已更新") {
        return Ok(BillingActionResult {
            message,
            password_changed: false,
        });
    }
    let updated_page = get_page_text(session, "/Self/service/consumeProtect").await?;
    let updated = dashboard_user(&updated_page).installment_flag;
    if updated.is_some_and(|value| (value - numeric).abs() < 0.000_1) {
        Ok(BillingActionResult {
            message: "消费保护限额已更新".to_string(),
            password_changed: false,
        })
    } else {
        Err(BillingError::ActionRejected(
            "计费系统没有确认消费限额已更新".to_string(),
        ))
    }
}

async fn bind_mac(
    session: &mut BillingSession,
    requested: Option<&str>,
) -> Result<BillingActionResult, BillingError> {
    let mac = validate_mac_input(requested)?;
    let _ = get_page_text(session, "/Self/service/myMac").await?;
    let text = post_form_action(
        session,
        "/Self/service/bindMac",
        "/Self/service/myMac",
        vec![
            ("macAddress".to_string(), mac.clone()),
            ("macType".to_string(), "0".to_string()),
        ],
        true,
    )
    .await?;
    let params = [
        ("pageSize", "100"),
        ("pageNumber", "1"),
        ("sortName", "2"),
        ("sortOrder", "DESC"),
    ];
    if let Ok(after) = get_ajax_text(
        session,
        "/Self/service/getMacList",
        &params,
        "/Self/service/myMac",
    )
    .await
    .and_then(|body| {
        parse_billing_table(
            &body,
            &["online", "mac", "device", "lastLoginAt", "lastIp"],
            "设备列表",
        )
    }) {
        if table_contains_mac(&after, &mac) {
            return Ok(BillingActionResult {
                message: "MAC 已绑定".to_string(),
                password_changed: false,
            });
        }
    }
    Ok(BillingActionResult {
        message: parse_action_response(&text, "MAC 已绑定")?,
        password_changed: false,
    })
}

async fn unbind_mac(
    session: &mut BillingSession,
    requested: Option<&str>,
) -> Result<BillingActionResult, BillingError> {
    let mac = validate_mac_input(requested)?;
    let params = [
        ("pageSize", "100"),
        ("pageNumber", "1"),
        ("sortName", "2"),
        ("sortOrder", "DESC"),
    ];
    let before = get_ajax_text(
        session,
        "/Self/service/getMacList",
        &params,
        "/Self/service/myMac",
    )
    .await
    .and_then(|text| {
        parse_billing_table(
            &text,
            &["online", "mac", "device", "lastLoginAt", "lastIp"],
            "设备列表",
        )
    })?;
    if !table_contains_mac(&before, &mac) {
        return Err(BillingError::ActionRejected(
            "设备列表已经变化，请刷新后重试".to_string(),
        ));
    }
    let ajax_csrf_token = session.ajax_csrf_token.as_deref().ok_or_else(|| {
        BillingError::Protocol("设备页面缺少安全操作令牌，请刷新后重试".to_string())
    })?;
    let mut url = same_origin_url(&format!("{BILLING_ORIGIN}/Self/service/unbindmac"))?;
    url.query_pairs_mut()
        .append_pair("mac", &mac)
        .append_pair("ajaxCsrfToken", ajax_csrf_token)
        .append_pair("t", &cache_buster().to_string());
    let referer = same_origin_url(&format!("{BILLING_ORIGIN}/Self/service/myMac"))?;
    let (final_url, response) = get_follow(
        &session.client,
        url,
        &mut session.cookies,
        Some(&referer),
        "text/html,application/xhtml+xml,*/*;q=0.8",
        "document",
    )
    .await?;
    if final_url.path().starts_with("/Self/login") {
        return Err(BillingError::AuthenticationRejected);
    }
    if !response.status().is_success() {
        return Err(BillingError::Protocol(format!(
            "解绑端点返回 HTTP {}",
            response.status().as_u16()
        )));
    }
    let _ = response.bytes().await.map_err(network_error)?;
    let after = get_ajax_text(
        session,
        "/Self/service/getMacList",
        &params,
        "/Self/service/myMac",
    )
    .await
    .and_then(|text| {
        parse_billing_table(
            &text,
            &["online", "mac", "device", "lastLoginAt", "lastIp"],
            "设备列表",
        )
    })?;
    if table_contains_mac(&after, &mac) {
        return Err(BillingError::ActionRejected(
            "计费系统没有确认设备已解除绑定".to_string(),
        ));
    }
    Ok(BillingActionResult {
        message: "设备已解除绑定".to_string(),
        password_changed: false,
    })
}

async fn change_password(
    session: &mut BillingSession,
    saved_password: &str,
    old_password: Option<&str>,
    new_password: Option<&str>,
) -> Result<BillingActionResult, BillingError> {
    let old_password = old_password
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BillingError::InvalidRequest("请输入当前密码".to_string()))?;
    if old_password != saved_password {
        return Err(BillingError::InvalidRequest(
            "当前密码与 App 中已保存的计费密码不一致".to_string(),
        ));
    }
    let page = get_page_text(session, "/Self/setting/changePassword").await?;
    let policy = parse_password_policy(&page);
    let new_password = validate_new_password(new_password, old_password, &policy)?;
    let csrf = form_csrf_token(&page)?;
    let text = post_form_action(
        session,
        "/Self/setting/changePasswordMethod",
        "/Self/setting/changePassword",
        vec![
            ("csrftoken".to_string(), csrf),
            ("oldPassword".to_string(), old_password.to_string()),
            ("newPassword".to_string(), new_password.to_string()),
            ("confirmPassword".to_string(), new_password.to_string()),
        ],
        false,
    )
    .await?;
    Ok(BillingActionResult {
        message: parse_action_response(&text, "密码修改成功")?,
        password_changed: true,
    })
}

async fn update_security_questions(
    session: &mut BillingSession,
    saved_password: &str,
    supplied_password: Option<&str>,
    questions: &[BillingQuestionAnswer],
) -> Result<BillingActionResult, BillingError> {
    let supplied_password = supplied_password
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BillingError::InvalidRequest("请输入当前密码".to_string()))?;
    if supplied_password != saved_password {
        return Err(BillingError::InvalidRequest(
            "当前密码与 App 中已保存的计费密码不一致".to_string(),
        ));
    }
    let page = get_page_text(session, "/Self/setting/passwordQuestion").await?;
    let allowed = parse_security_questions(&page);
    validate_question_answers(questions, &allowed)?;
    let csrf = form_csrf_token(&page)?;
    let mut fields = vec![
        ("csrftoken".to_string(), csrf),
        ("password".to_string(), supplied_password.to_string()),
    ];
    for (index, question) in questions.iter().enumerate() {
        fields.push((
            format!("question{}", index + 1),
            question.question_id.clone(),
        ));
        fields.push((format!("answer{}", index + 1), question.answer.clone()));
    }
    let text = post_form_action(
        session,
        "/Self/setting/updatePasswordQuestion",
        "/Self/setting/passwordQuestion",
        fields,
        false,
    )
    .await?;
    Ok(BillingActionResult {
        message: parse_action_response(&text, "密码保护已更新")?,
        password_changed: false,
    })
}

pub(crate) async fn disconnect_session(
    account: &str,
    password: &str,
    compatibility: VpnCompatibility,
    session_id: &str,
    ip: &str,
    mac: &str,
) -> Result<String, BillingError> {
    validate_session_action(session_id, ip, mac)?;
    let mut session = authenticate(account, password, compatibility).await?;
    let result = async {
        let online_text = get_dashboard_text(
            &mut session,
            "/Self/dashboard/getOnlineList",
            "application/json,*/*;q=0.8",
        )
        .await?;
        let online_sessions = parse_online_sessions(&online_text)?;
        let requested_mac = normalize_mac_for_action(mac);
        let exists = online_sessions.iter().any(|item| {
            item.session_id == session_id
                && item.ip == ip
                && normalize_mac_for_action(&item.mac) == requested_mac
        });
        if !exists {
            return Err(BillingError::ActionRejected(
                "在线会话已经变化，请刷新计费中心后重试".to_string(),
            ));
        }

        let mut action_url =
            same_origin_url(&format!("{BILLING_ORIGIN}/Self/dashboard/tooffline"))?;
        let ajax_csrf_token = session.ajax_csrf_token.as_deref().ok_or_else(|| {
            BillingError::Protocol("控制台缺少安全操作令牌，请刷新后重试".to_string())
        })?;
        action_url
            .query_pairs_mut()
            .append_pair("sessionid", session_id)
            .append_pair("ip", ip)
            .append_pair("mac", &requested_mac)
            .append_pair("ajaxCsrfToken", ajax_csrf_token)
            .append_pair("t", &cache_buster().to_string());
        let referer = session.dashboard_url.clone();
        let (_, response) = get_follow(
            &session.client,
            action_url,
            &mut session.cookies,
            Some(&referer),
            "application/json,*/*;q=0.8",
            "empty",
        )
        .await?;
        let response_text = response.text().await.map_err(network_error)?;
        let response_json: serde_json::Value = serde_json::from_str(&response_text)
            .map_err(|_| BillingError::Protocol("注销响应不是有效 JSON".to_string()))?;
        if response_json
            .get("success")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            Ok("在线会话已注销".to_string())
        } else {
            Err(BillingError::ActionRejected(
                "计费系统未能注销该会话".to_string(),
            ))
        }
    }
    .await;
    logout(&mut session).await;
    result
}

pub(crate) async fn set_mauth_enabled(
    account: &str,
    password: &str,
    compatibility: VpnCompatibility,
    enabled: bool,
) -> Result<String, BillingError> {
    let mut session = authenticate(account, password, compatibility).await?;
    let result = async {
        let current = fetch_mauth_state(&mut session).await?;
        if current == enabled {
            return Ok(if enabled {
                "无感认证已经开启".to_string()
            } else {
                "无感认证已经关闭".to_string()
            });
        }

        let mut action_url = same_origin_url(&format!(
            "{BILLING_ORIGIN}/Self/dashboard/oprateMauthAction"
        ))?;
        action_url
            .query_pairs_mut()
            .append_pair("t", &cache_buster().to_string());
        let referer = session.dashboard_url.clone();
        let (_, response) = get_follow(
            &session.client,
            action_url,
            &mut session.cookies,
            Some(&referer),
            "text/html,application/xhtml+xml,*/*;q=0.8",
            "document",
        )
        .await?;
        let _ = response.bytes().await.map_err(network_error)?;

        let actual = fetch_mauth_state(&mut session).await?;
        if actual != enabled {
            return Err(BillingError::ActionRejected(
                "无感认证状态未按预期更新".to_string(),
            ));
        }
        Ok(if enabled {
            "无感认证已开启".to_string()
        } else {
            "无感认证已关闭".to_string()
        })
    }
    .await;
    logout(&mut session).await;
    result
}

async fn authenticate(
    account: &str,
    password: &str,
    compatibility: VpnCompatibility,
) -> Result<BillingSession, BillingError> {
    if account.trim().is_empty() || password.is_empty() {
        return Err(BillingError::InvalidRequest(
            "首选账号缺少已保存的账号或密码".to_string(),
        ));
    }

    let client = build_client(compatibility).await?;
    let mut cookies = SessionCookies::default();
    let login_url = Url::parse(BILLING_LOGIN_URL)
        .map_err(|_| BillingError::Protocol("登录地址无效".to_string()))?;
    let (login_effective_url, login_response) = get_follow(
        &client,
        login_url,
        &mut cookies,
        None,
        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        "document",
    )
    .await?;
    let login_html = login_response.text().await.map_err(network_error)?;

    let checkcode = input_value(&login_html, "checkcode")
        .ok_or_else(|| BillingError::Protocol("登录页缺少 checkcode".to_string()))?;
    if checkcode.is_empty()
        || checkcode.len() > 4
        || !checkcode.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(BillingError::Protocol(
            "登录页 checkcode 格式不受支持".to_string(),
        ));
    }
    if captcha_required(&login_html)? {
        return Err(BillingError::CaptchaRequired);
    }
    let verify_url = login_action(&login_html, &login_effective_url)?;

    // The service enables its normal login path only after the browser has
    // loaded the page resources and requested randomCode. These requests are
    // session-local, read-only, and bounded to the same HTTPS origin.
    replay_login_assets(&client, &mut cookies, &login_html, &login_effective_url).await;

    let cache_buster = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let random_code_url = same_origin_url(&format!(
        "{BILLING_ORIGIN}/Self/login/randomCode?t=0.{cache_buster}"
    ))?;
    let (_, random_code_response) = get_follow(
        &client,
        random_code_url,
        &mut cookies,
        Some(&login_effective_url),
        "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8",
        "image",
    )
    .await?;
    let _ = random_code_response.bytes().await.map_err(network_error)?;

    let brand_url = same_origin_url(&format!(
        "{BILLING_ORIGIN}/Self/login/getBrandInfo?t=0.{cache_buster}"
    ))?;
    if let Ok((_, response)) = get_follow(
        &client,
        brand_url,
        &mut cookies,
        Some(&login_effective_url),
        "*/*",
        "empty",
    )
    .await
    {
        let _ = response.bytes().await;
    }

    let (dashboard_url, dashboard_html) = post_login(
        &client,
        verify_url,
        &login_effective_url,
        &mut cookies,
        &checkcode,
        account,
        password,
    )
    .await?;

    if login_form_present(&dashboard_html, &dashboard_url) {
        return if dashboard_html.contains("验证码")
            || dashboard_html.contains("randomDiv\" class=\"form-group\"")
        {
            Err(BillingError::CaptchaRequired)
        } else {
            Err(BillingError::AuthenticationRejected)
        };
    }
    if !dashboard_url.path().starts_with("/Self/dashboard") && !dashboard_html.contains("账户余额")
    {
        return Err(BillingError::Protocol("登录后未进入计费控制台".to_string()));
    }

    let ajax_csrf_token =
        fetch_page_csrf_token(&client, &mut cookies, &dashboard_html, &dashboard_url).await;

    Ok(BillingSession {
        client,
        cookies,
        dashboard_url,
        dashboard_html,
        ajax_csrf_token,
    })
}

async fn replay_login_assets(
    client: &Client,
    cookies: &mut SessionCookies,
    login_html: &str,
    login_url: &Url,
) {
    let base_cookies = cookies.clone();
    let assets = login_assets(login_html, login_url);
    for batch in assets.chunks(6) {
        let mut pending = FuturesUnordered::new();
        for (asset_url, destination) in batch.iter().cloned() {
            let client = client.clone();
            let mut request_cookies = base_cookies.clone();
            let referer = login_url.clone();
            pending.push(async move {
                let request = async {
                    let (_, response) = get_follow(
                        &client,
                        asset_url,
                        &mut request_cookies,
                        Some(&referer),
                        "*/*",
                        destination,
                    )
                    .await?;
                    let _ = response.bytes().await.map_err(network_error)?;
                    Ok::<SessionCookies, BillingError>(request_cookies)
                };
                tokio::time::timeout(Duration::from_secs(6), request)
                    .await
                    .ok()
                    .and_then(Result::ok)
            });
        }

        while let Some(request_cookies) = pending.next().await {
            if let Some(request_cookies) = request_cookies {
                cookies.merge(request_cookies);
            }
        }
    }
}

async fn logout(session: &mut BillingSession) {
    // Logging out only ends this temporary billing session. It does not
    // disconnect the campus network or change any account setting.
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        if let Ok(logout_url) = Url::parse(BILLING_LOGOUT_URL) {
            if let Ok((_, response)) = get_follow(
                &session.client,
                logout_url,
                &mut session.cookies,
                Some(&session.dashboard_url),
                "text/html,*/*;q=0.8",
                "document",
            )
            .await
            {
                let _ = response.bytes().await;
            }
        }
    })
    .await;
}

fn cache_buster() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

async fn get_dashboard_text(
    session: &mut BillingSession,
    path: &str,
    accept: &str,
) -> Result<String, BillingError> {
    let mut url = same_origin_url(&format!("{BILLING_ORIGIN}{path}"))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("t", &cache_buster().to_string());
    }
    let referer = session.dashboard_url.clone();
    let (final_url, response) = get_follow(
        &session.client,
        url,
        &mut session.cookies,
        Some(&referer),
        accept,
        "empty",
    )
    .await?;
    if final_url.path().starts_with("/Self/login") {
        return Err(BillingError::AuthenticationRejected);
    }
    if !response.status().is_success() {
        return Err(BillingError::Protocol(format!(
            "数据端点返回 HTTP {}",
            response.status().as_u16()
        )));
    }
    response.text().await.map_err(network_error)
}

async fn get_page_text(session: &mut BillingSession, path: &str) -> Result<String, BillingError> {
    let url = same_origin_url(&format!("{BILLING_ORIGIN}{path}"))?;
    let referer = session.dashboard_url.clone();
    let (final_url, response) = get_follow(
        &session.client,
        url,
        &mut session.cookies,
        Some(&referer),
        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        "document",
    )
    .await?;
    if !response.status().is_success() {
        return Err(BillingError::Protocol(format!(
            "页面端点返回 HTTP {}",
            response.status().as_u16()
        )));
    }
    let text = response.text().await.map_err(network_error)?;
    if final_url.path().starts_with("/Self/login") || login_form_present(&text, &final_url) {
        return Err(BillingError::AuthenticationRejected);
    }
    session.ajax_csrf_token =
        fetch_page_csrf_token(&session.client, &mut session.cookies, &text, &final_url).await;
    Ok(text)
}

async fn get_page_or_warning(
    session: &mut BillingSession,
    path: &str,
    label: &str,
    warnings: &mut Vec<String>,
) -> String {
    match get_page_text(session, path).await {
        Ok(html) => html,
        Err(error) => {
            warnings.push(format!("{label}读取失败：{}", error.user_message()));
            String::new()
        }
    }
}

async fn fetch_page_table_or_warning(
    session: &mut BillingSession,
    page_path: &str,
    table_path: &str,
    params: &[(&str, &str)],
    fields: &[&str],
    table_label: &str,
    warnings: &mut Vec<String>,
) -> (String, BillingTable) {
    let html = match get_page_text(session, page_path).await {
        Ok(html) => html,
        Err(error) => {
            warnings.push(format!(
                "{table_label}页面读取失败：{}",
                error.user_message()
            ));
            return (String::new(), BillingTable::default());
        }
    };
    let table = fetch_table_or_warning(
        session,
        table_path,
        params,
        page_path,
        fields,
        table_label,
        warnings,
    )
    .await;
    (html, table)
}

async fn fetch_page_table_bounded(
    session: &mut BillingSession,
    page_path: &str,
    table_path: &str,
    params: &[(&str, &str)],
    fields: &[&str],
    table_label: &str,
    warnings: &mut Vec<String>,
) -> (String, BillingTable) {
    let request = fetch_page_table_or_warning(
        session,
        page_path,
        table_path,
        params,
        fields,
        table_label,
        warnings,
    );
    let (html, table) = match tokio::time::timeout(Duration::from_secs(10), request).await {
        Ok(result) => result,
        Err(_) => {
            warnings.push(format!("{table_label}读取超过 10 秒，已跳过"));
            (String::new(), BillingTable::default())
        }
    };
    (html, table)
}

async fn get_page_bounded(
    session: &mut BillingSession,
    path: &str,
    label: &str,
    warnings: &mut Vec<String>,
) -> String {
    let request = get_page_or_warning(session, path, label, warnings);
    let html = match tokio::time::timeout(Duration::from_secs(8), request).await {
        Ok(html) => html,
        Err(_) => {
            warnings.push(format!("{label}读取超过 8 秒，已跳过"));
            String::new()
        }
    };
    html
}

async fn get_ajax_text(
    session: &mut BillingSession,
    path: &str,
    params: &[(&str, &str)],
    referer_path: &str,
) -> Result<String, BillingError> {
    let mut url = same_origin_url(&format!("{BILLING_ORIGIN}{path}"))?;
    {
        let mut query = url.query_pairs_mut();
        for (name, value) in params {
            query.append_pair(name, value);
        }
        query.append_pair("t", &cache_buster().to_string());
    }
    let referer = same_origin_url(&format!("{BILLING_ORIGIN}{referer_path}"))?;
    let (final_url, response) = get_follow(
        &session.client,
        url,
        &mut session.cookies,
        Some(&referer),
        "application/json,text/plain,*/*;q=0.8",
        "empty",
    )
    .await?;
    if final_url.path().starts_with("/Self/login") {
        return Err(BillingError::AuthenticationRejected);
    }
    if !response.status().is_success() {
        return Err(BillingError::Protocol(format!(
            "数据端点返回 HTTP {}",
            response.status().as_u16()
        )));
    }
    response.text().await.map_err(network_error)
}

async fn get_ajax_text_owned(
    session: &mut BillingSession,
    path: &str,
    params: &[(String, String)],
    referer_path: &str,
) -> Result<String, BillingError> {
    let borrowed = params
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect::<Vec<_>>();
    get_ajax_text(session, path, &borrowed, referer_path).await
}

async fn fetch_active_package_reservation(
    session: &mut BillingSession,
) -> Result<Option<ActivePackageReservation>, BillingError> {
    // The package page omits the current package from its selectable cards and
    // does not consistently print the pending package beside the cancel
    // control. The first package-log row is the canonical state returned by
    // the site after both doPackage and undoPackage.
    let params = [
        ("pageSize", "10"),
        ("pageNumber", "1"),
        ("sortName", "fldchangedate"),
        ("sortOrder", "DESC"),
    ];
    let text = get_ajax_text(
        session,
        "/Self/service/packageLog",
        &params,
        "/Self/service/package",
    )
    .await?;
    parse_active_package_reservation(&text)
}

async fn fetch_active_package_reservation_bounded(
    session: &mut BillingSession,
    warnings: &mut Vec<String>,
) -> Option<ActivePackageReservation> {
    match tokio::time::timeout(
        Duration::from_secs(8),
        fetch_active_package_reservation(session),
    )
    .await
    {
        Ok(Ok(reservation)) => reservation,
        Ok(Err(error)) => {
            warnings.push(format!("下一周期套餐读取失败：{}", error.user_message()));
            None
        }
        Err(_) => {
            warnings.push("下一周期套餐读取超过 8 秒，已跳过".to_string());
            None
        }
    }
}

async fn fetch_table_or_warning(
    session: &mut BillingSession,
    path: &str,
    params: &[(&str, &str)],
    referer_path: &str,
    array_fields: &[&str],
    label: &str,
    warnings: &mut Vec<String>,
) -> BillingTable {
    match get_ajax_text(session, path, params, referer_path)
        .await
        .and_then(|text| parse_billing_table(&text, array_fields, label))
    {
        Ok(table) => table,
        Err(error) => {
            warnings.push(format!("{label}读取失败：{}", error.user_message()));
            BillingTable::default()
        }
    }
}

async fn post_form_action(
    session: &mut BillingSession,
    path: &str,
    referer_path: &str,
    mut fields: Vec<(String, String)>,
    require_ajax_token: bool,
) -> Result<String, BillingError> {
    let url = same_origin_url(&format!("{BILLING_ORIGIN}{path}"))?;
    let referer = same_origin_url(&format!("{BILLING_ORIGIN}{referer_path}"))?;
    if let Some(token) = session.ajax_csrf_token.as_deref() {
        fields.push(("ajaxCsrfToken".to_string(), token.to_string()));
    } else if require_ajax_token {
        return Err(BillingError::Protocol(
            "页面缺少安全操作令牌，请刷新后重试".to_string(),
        ));
    }
    fields.push(("t".to_string(), cache_buster().to_string()));
    let mut request = session
        .client
        .post(url.clone())
        .header(ACCEPT, "application/json,text/plain,*/*;q=0.8")
        .header(ORIGIN, BILLING_ORIGIN)
        .header(REFERER, referer.as_str())
        .header("X-Requested-With", "XMLHttpRequest")
        .header("Sec-Fetch-Dest", "empty")
        .header("Sec-Fetch-Mode", "cors")
        .header("Sec-Fetch-Site", "same-origin")
        .form(&fields);
    if let Some(value) = session.cookies.header() {
        request = request.header(COOKIE, value);
    }
    let response = request.send().await.map_err(network_error)?;
    session.cookies.absorb(response.headers());
    if response.status().is_redirection() {
        if matches!(response.status().as_u16(), 307 | 308) {
            return Err(BillingError::Protocol(
                "操作端点要求重复提交敏感表单，已安全中止".to_string(),
            ));
        }
        let next = redirect_target(&url, &response)?;
        let (_, response) = get_follow(
            &session.client,
            next,
            &mut session.cookies,
            Some(&url),
            "text/html,application/xhtml+xml,*/*;q=0.8",
            "document",
        )
        .await?;
        return response.text().await.map_err(network_error);
    }
    if !response.status().is_success() {
        return Err(BillingError::Protocol(format!(
            "操作端点返回 HTTP {}",
            response.status().as_u16()
        )));
    }
    response.text().await.map_err(network_error)
}

fn parse_action_response(text: &str, default_message: &str) -> Result<String, BillingError> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|_| BillingError::Protocol("操作响应不是有效 JSON".to_string()))?;
    let object = value
        .as_object()
        .ok_or_else(|| BillingError::Protocol("操作响应不是对象".to_string()))?;
    let state = object
        .get("state")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let success = state.eq_ignore_ascii_case("success")
        || object.get("success").and_then(serde_json::Value::as_bool) == Some(true);
    let message = object
        .get("message")
        .or_else(|| object.get("data"))
        .map(json_display)
        .filter(|value| value != "--" && !value.is_empty())
        .unwrap_or_else(|| default_message.to_string());
    if success {
        Ok(message)
    } else {
        Err(BillingError::ActionRejected(message))
    }
}

fn dashboard_user(html: &str) -> DashboardUser {
    embedded_user_json(html)
        .and_then(|json| serde_json::from_str::<DashboardUser>(json).ok())
        .unwrap_or_default()
}

fn form_csrf_token(html: &str) -> Result<String, BillingError> {
    let token = input_value(html, "csrftoken")
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .filter(|value| {
            value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
        .ok_or_else(|| BillingError::Protocol("页面缺少有效的表单安全令牌".to_string()))?;
    Ok(token)
}

fn action_csrf_token(html: &str, action: &str) -> Result<String, BillingError> {
    if action.is_empty()
        || action.len() > 64
        || !action
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(BillingError::Protocol("套餐操作入口无效".to_string()));
    }
    let lower = html.to_ascii_lowercase();
    let action = action.to_ascii_lowercase();
    for quote in ['\'', '"'] {
        let marker = format!("post({quote}{action}{quote}");
        let mut cursor = 0usize;
        while let Some(relative) = lower[cursor..].find(&marker) {
            let start = cursor + relative;
            let mut end = start.saturating_add(1_024).min(html.len());
            while end > start && !html.is_char_boundary(end) {
                end -= 1;
            }
            if let Some(token) = extract_literal_token(&html[start..end], "csrftoken") {
                return Ok(token);
            }
            cursor = start + marker.len();
        }
    }
    Err(BillingError::Protocol(
        "套餐操作入口缺少有效的专用安全令牌".to_string(),
    ))
}

fn validate_consume_limit(value: Option<&str>) -> Result<String, BillingError> {
    let value = value
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 12)
        .ok_or_else(|| BillingError::InvalidRequest("请输入消费限额".to_string()))?;
    let mut dot = false;
    let mut fraction_digits = 0usize;
    for byte in value.bytes() {
        if byte == b'.' && !dot {
            dot = true;
        } else if byte.is_ascii_digit() {
            if dot {
                fraction_digits += 1;
            }
        } else {
            return Err(BillingError::InvalidRequest(
                "消费限额必须是非负数字，最多三位小数".to_string(),
            ));
        }
    }
    if value.starts_with('.')
        || value.ends_with('.')
        || fraction_digits > 3
        || value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .is_none()
    {
        return Err(BillingError::InvalidRequest(
            "消费限额必须是非负数字，最多三位小数".to_string(),
        ));
    }
    let numeric = value.parse::<f64>().unwrap_or(-1.0);
    if !(0.0..=999_999.0).contains(&numeric) {
        return Err(BillingError::InvalidRequest(
            "消费限额必须在 0 到 999999 之间".to_string(),
        ));
    }
    Ok(value.to_string())
}

fn validate_mac_input(value: Option<&str>) -> Result<String, BillingError> {
    let normalized = normalize_mac_for_action(value.unwrap_or_default());
    if normalized.len() != 12 || !normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(BillingError::InvalidRequest(
            "MAC 地址必须由 12 位十六进制字符组成".to_string(),
        ));
    }
    Ok(normalized)
}

fn table_contains_mac(table: &BillingTable, expected: &str) -> bool {
    table.rows.iter().any(|row| {
        row.get("mac")
            .or_else(|| row.get("1"))
            .is_some_and(|value| normalize_mac_for_action(value) == expected)
    })
}

fn validate_new_password<'a>(
    value: Option<&'a str>,
    old_password: &str,
    policy: &BillingPasswordPolicy,
) -> Result<&'a str, BillingError> {
    let value = value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BillingError::InvalidRequest("请输入新密码".to_string()))?;
    let length = value.chars().count();
    if length < policy.min_length || length > policy.max_length {
        return Err(BillingError::InvalidRequest(format!(
            "新密码长度必须为 {}–{} 位",
            policy.min_length, policy.max_length
        )));
    }
    if value == old_password {
        return Err(BillingError::InvalidRequest(
            "新密码不能与当前密码相同".to_string(),
        ));
    }
    if value.chars().any(char::is_whitespace)
        || policy.require_uppercase
            && !value
                .chars()
                .any(|character| character.is_ascii_uppercase())
        || policy.require_lowercase
            && !value
                .chars()
                .any(|character| character.is_ascii_lowercase())
        || policy.require_digit && !value.chars().any(|character| character.is_ascii_digit())
        || policy.require_special
            && !value
                .chars()
                .any(|character| "!@#$%^&*()".contains(character))
    {
        return Err(BillingError::InvalidRequest(
            "新密码不符合计费系统当前密码策略".to_string(),
        ));
    }
    Ok(value)
}

fn validate_question_answers(
    answers: &[BillingQuestionAnswer],
    allowed: &[BillingSecurityQuestion],
) -> Result<(), BillingError> {
    if answers.len() != 3 {
        return Err(BillingError::InvalidRequest(
            "必须设置三个密码保护问题".to_string(),
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for answer in answers {
        if !allowed.iter().any(|item| item.id == answer.question_id)
            || !seen.insert(answer.question_id.as_str())
        {
            return Err(BillingError::InvalidRequest(
                "密码保护问题无效或存在重复".to_string(),
            ));
        }
        let length = answer.answer.chars().count();
        if length == 0 || length > 16 || answer.answer.chars().any(char::is_control) {
            return Err(BillingError::InvalidRequest(
                "每个密码保护答案必须为 1–16 个字符".to_string(),
            ));
        }
    }
    Ok(())
}

async fn populate_dashboard_details<P>(
    session: &mut BillingSession,
    snapshot: &mut BillingSnapshot,
    progress: &P,
) where
    P: Fn(&str, u8) + Send + Sync,
{
    // These endpoints are quick in normal operation, but must remain
    // sequential for the same authenticated Dr.COM session. Bound every
    // optional request so one unavailable endpoint cannot block the overview.
    progress("账户概览已读取，正在读取近期上网记录", 28);
    let login_history = match tokio::time::timeout(Duration::from_secs(6), async {
        get_dashboard_text(
            session,
            "/Self/dashboard/getLoginHistory",
            "application/json,*/*;q=0.8",
        )
        .await
        .and_then(|text| parse_login_history(&text))
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(BillingError::Network(
            "最近上网记录请求超过 6 秒".to_string(),
        )),
    };
    progress("正在读取当前在线会话", 38);
    let online_sessions = match tokio::time::timeout(Duration::from_secs(6), async {
        get_dashboard_text(
            session,
            "/Self/dashboard/getOnlineList",
            "application/json,*/*;q=0.8",
        )
        .await
        .and_then(|text| parse_online_sessions(&text))
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(BillingError::Network("在线会话请求超过 6 秒".to_string())),
    };
    progress("正在读取会话注销提示", 48);
    let offline_tip = match tokio::time::timeout(Duration::from_secs(6), async {
        get_dashboard_text(
            session,
            "/Self/dashboard/getOfflineTip",
            "text/plain,*/*;q=0.8",
        )
        .await
        .and_then(|text| parse_offline_tip(&text))
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(BillingError::Network("注销提示请求超过 6 秒".to_string())),
    };
    progress("正在读取无感认证状态", 58);
    let mauth_enabled =
        match tokio::time::timeout(Duration::from_secs(6), fetch_mauth_state(session)).await {
            Ok(result) => result,
            Err(_) => Err(BillingError::Network(
                "无感认证状态请求超过 6 秒".to_string(),
            )),
        };

    match login_history {
        Ok(records) => snapshot.login_history = records,
        Err(error) => snapshot
            .warnings
            .push(format!("最近上网记录读取失败：{}", error.user_message())),
    }

    match online_sessions {
        Ok(records) => snapshot.online_sessions = records,
        Err(error) => snapshot
            .warnings
            .push(format!("在线会话读取失败：{}", error.user_message())),
    }

    match offline_tip {
        Ok(tip) => snapshot.offline_tip = tip,
        Err(error) => snapshot
            .warnings
            .push(format!("注销提示读取失败：{}", error.user_message())),
    }

    match mauth_enabled {
        Ok(enabled) => snapshot.mauth_enabled = Some(enabled),
        Err(error) => snapshot
            .warnings
            .push(format!("无感认证状态读取失败：{}", error.user_message())),
    }
}

async fn fetch_mauth_state(session: &mut BillingSession) -> Result<bool, BillingError> {
    let text = get_dashboard_text(
        session,
        "/Self/dashboard/refreshMauthType",
        "application/json,*/*;q=0.8",
    )
    .await?;
    parse_mauth_state(&text)
}

fn validate_session_action(session_id: &str, ip: &str, mac: &str) -> Result<(), BillingError> {
    if session_id.is_empty()
        || session_id.len() > 512
        || session_id.chars().any(char::is_control)
        || IpAddr::from_str(ip).is_err()
    {
        return Err(BillingError::InvalidRequest(
            "在线会话参数格式无效，请刷新后重试".to_string(),
        ));
    }
    let normalized_mac = normalize_mac_for_action(mac);
    if !normalized_mac.is_empty()
        && (normalized_mac.len() != 12
            || !normalized_mac.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(BillingError::InvalidRequest(
            "在线会话的 MAC 地址格式无效，请刷新后重试".to_string(),
        ));
    }
    Ok(())
}

fn normalize_mac_for_action(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_hexdigit())
        .flat_map(char::to_uppercase)
        .collect()
}

async fn build_client(compatibility: VpnCompatibility) -> Result<Client, BillingError> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en;q=0.6".parse().unwrap());
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .use_rustls_tls()
        .user_agent(BILLING_USER_AGENT)
        .default_headers(headers);

    if !matches!(compatibility, VpnCompatibility::Minimum) {
        let addresses = if compatibility == VpnCompatibility::Low {
            tokio::task::spawn_blocking(|| query_campus_dns_ipv4(BILLING_HOST))
                .await
                .map_err(|error| BillingError::Network(format!("校园网 DNS 任务失败：{error}")))?
                .map_err(BillingError::Network)?
        } else {
            vec![BILLING_FIXED_ADDRESS]
        };
        let socket_addresses = addresses
            .into_iter()
            .map(|address| SocketAddr::new(IpAddr::V4(address), 443))
            .collect::<Vec<_>>();
        builder = builder.resolve_to_addrs(BILLING_HOST, &socket_addresses);
    }

    builder.build().map_err(network_error)
}

async fn build_account_discovery_client(
    compatibility: VpnCompatibility,
) -> Result<Client, BillingError> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en;q=0.6".parse().unwrap());
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .use_rustls_tls()
        .user_agent(BILLING_USER_AGENT)
        .default_headers(headers);

    if !matches!(compatibility, VpnCompatibility::Minimum) {
        let (lgn_addresses, billing_addresses) = if compatibility == VpnCompatibility::Low {
            let lgn = tokio::task::spawn_blocking(|| query_campus_dns_ipv4(LGN_HOST))
                .await
                .map_err(|error| BillingError::Network(format!("校园网 DNS 任务失败：{error}")))?
                .map_err(BillingError::Network)?;
            let billing = tokio::task::spawn_blocking(|| query_campus_dns_ipv4(BILLING_HOST))
                .await
                .map_err(|error| BillingError::Network(format!("校园网 DNS 任务失败：{error}")))?
                .map_err(BillingError::Network)?;
            (lgn, billing)
        } else {
            (
                vec![
                    Ipv4Addr::new(172, 30, 201, 2),
                    Ipv4Addr::new(172, 30, 201, 10),
                ],
                vec![BILLING_FIXED_ADDRESS],
            )
        };
        let lgn_sockets = lgn_addresses
            .into_iter()
            .map(|address| SocketAddr::new(IpAddr::V4(address), ACCOUNT_DISCOVERY_PORT))
            .collect::<Vec<_>>();
        let billing_sockets = billing_addresses
            .into_iter()
            .map(|address| SocketAddr::new(IpAddr::V4(address), 443))
            .collect::<Vec<_>>();
        builder = builder
            .resolve_to_addrs(LGN_HOST, &lgn_sockets)
            .resolve_to_addrs(BILLING_HOST, &billing_sockets);
    }

    builder.build().map_err(network_error)
}

fn account_discovery_url() -> Result<Url, BillingError> {
    let mut url = Url::parse(&format!(
        "https://{LGN_HOST}:{ACCOUNT_DISCOVERY_PORT}{ACCOUNT_DISCOVERY_PATH}"
    ))
    .map_err(|_| BillingError::Protocol("校园网账号发现地址无效".to_string()))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string();
    url.query_pairs_mut()
        .append_pair("callback", "726427262622")
        .append_pair("self_type", "27")
        .append_pair("user_account", "")
        .append_pair("user_password", "")
        .append_pair("wlan_user_mac", "")
        .append_pair("jsVersion", "2238243824")
        .append_pair("program_index", "79225954737327212323222f212e2723")
        .append_pair("page_index", "755e577b7c4e27212323222f212e2320")
        .append_pair("encrypt", "1")
        .append_pair("v", &nonce)
        .append_pair("lang", "zh");
    Ok(url)
}

fn validate_account_discovery_url(url: &Url) -> Result<(), BillingError> {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(BillingError::Protocol(
            "账号发现流程尝试访问非受信任地址".to_string(),
        ));
    }
    match (url.host_str(), url.port_or_known_default(), url.path()) {
        (Some(host), Some(ACCOUNT_DISCOVERY_PORT), ACCOUNT_DISCOVERY_PATH) if host == LGN_HOST => {
            Ok(())
        }
        (Some(BILLING_HOST), Some(443), BILLING_EPORTAL_LOGIN_PATH) => Ok(()),
        (Some(BILLING_HOST), Some(443), path)
            if path == BILLING_DASHBOARD_PATH
                || path.starts_with(&format!("{BILLING_DASHBOARD_PATH}/")) =>
        {
            Ok(())
        }
        _ => Err(BillingError::Protocol(
            "账号发现流程尝试跳转到非受信任地址".to_string(),
        )),
    }
}

fn parse_self_auth_url(source: &str) -> Result<Option<Url>, BillingError> {
    let trimmed = source.trim().trim_start_matches('\u{feff}');
    let Some(open) = trimmed.find('(') else {
        return Err(BillingError::Protocol(
            "自服务响应缺少 JSON 包装".to_string(),
        ));
    };
    let Some(close) = trimmed.rfind(')') else {
        return Err(BillingError::Protocol(
            "自服务响应缺少 JSON 结尾".to_string(),
        ));
    };
    if close <= open || !trimmed[..open].trim().starts_with("dr1004") {
        return Err(BillingError::Protocol("自服务响应包装不受支持".to_string()));
    }
    let value: serde_json::Value = serde_json::from_str(&trimmed[open + 1..close])
        .map_err(|_| BillingError::Protocol("自服务响应 JSON 无法解析".to_string()))?;
    if value.get("result").and_then(serde_json::Value::as_i64) != Some(1) {
        return Ok(None);
    }
    let raw = value
        .get("self_auth_url")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| BillingError::Protocol("自服务响应缺少登录地址".to_string()))?;
    let url =
        Url::parse(raw).map_err(|_| BillingError::Protocol("自服务登录地址无效".to_string()))?;
    validate_account_discovery_url(&url)?;
    if url.path() != BILLING_EPORTAL_LOGIN_PATH
        || !["account", "timestamp", "sign"].iter().all(|name| {
            url.query_pairs()
                .any(|(key, value)| key == *name && !value.is_empty())
        })
    {
        return Err(BillingError::Protocol(
            "自服务登录地址缺少必要的签名参数".to_string(),
        ));
    }
    Ok(Some(url))
}

async fn limited_response_text(
    response: Response,
    label: &str,
    limit: usize,
) -> Result<String, BillingError> {
    let bytes = response.bytes().await.map_err(network_error)?;
    if bytes.len() > limit {
        return Err(BillingError::Protocol(format!("{label}内容过大")));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn account_discovery_get_follow(
    client: &Client,
    mut url: Url,
    cookies: &mut HostCookies,
    referer: Option<&Url>,
) -> Result<(Url, String), BillingError> {
    let mut current_referer = referer.map(Url::to_string);
    for _ in 0..=MAX_REDIRECTS {
        validate_account_discovery_url(&url)?;
        let mut request = client
            .get(url.clone())
            .header(
                ACCEPT,
                "text/html,application/xhtml+xml,application/json;q=0.9,*/*;q=0.8",
            )
            .header("Sec-Fetch-Dest", "document")
            .header("Sec-Fetch-Mode", "navigate")
            .header(
                "Sec-Fetch-Site",
                if current_referer.is_some() {
                    "same-site"
                } else {
                    "none"
                },
            );
        if let Some(value) = cookies.header(&url) {
            request = request.header(COOKIE, value);
        }
        if let Some(value) = current_referer.as_deref() {
            request = request.header(REFERER, value);
        }
        let response = request.send().await.map_err(network_error)?;
        cookies.absorb(&url, response.headers());
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| BillingError::Protocol("账号发现重定向缺少 Location".to_string()))?;
            let next = url
                .join(location)
                .map_err(|_| BillingError::Protocol("账号发现重定向地址无效".to_string()))?;
            validate_account_discovery_url(&next)?;
            current_referer = Some(url.to_string());
            url = next;
            continue;
        }
        if !response.status().is_success() {
            return Err(BillingError::Protocol(format!(
                "账号发现服务返回 HTTP {}",
                response.status().as_u16()
            )));
        }
        let text = limited_response_text(response, "账号发现页面", 2 * 1024 * 1024).await?;
        return Ok((url, text));
    }
    Err(BillingError::Protocol("账号发现重定向次数过多".to_string()))
}

fn parse_discovered_account(html: &str) -> Result<DiscoveredCampusAccount, BillingError> {
    let json = embedded_user_json(html)
        .ok_or_else(|| BillingError::Protocol("dashboard 缺少用户数据".to_string()))?;
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|_| BillingError::Protocol("dashboard 用户数据无法解析".to_string()))?;
    let user = value
        .get("userName")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| {
            (5..=20).contains(&value.len())
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
        .ok_or_else(|| BillingError::Protocol("dashboard 账号格式无效".to_string()))?;
    let pass = value
        .get("userPassword")
        .and_then(serde_json::Value::as_str)
        .filter(|value| {
            !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
        })
        .ok_or_else(|| BillingError::Protocol("dashboard 密码字段无效".to_string()))?;
    Ok(DiscoveredCampusAccount {
        user: user.to_string(),
        pass: pass.to_string(),
    })
}

pub(crate) async fn discover_current_campus_account(
    compatibility: VpnCompatibility,
) -> Result<Option<DiscoveredCampusAccount>, BillingError> {
    let client = build_account_discovery_client(compatibility).await?;
    let discovery_url = account_discovery_url()?;
    validate_account_discovery_url(&discovery_url)?;
    let response = client
        .get(discovery_url.clone())
        .header(ACCEPT, "application/javascript,text/javascript,*/*;q=0.8")
        .send()
        .await
        .map_err(network_error)?;
    if !response.status().is_success() {
        return Err(BillingError::Protocol(format!(
            "自服务接口返回 HTTP {}",
            response.status().as_u16()
        )));
    }
    let source = limited_response_text(response, "自服务响应", 256 * 1024).await?;
    let Some(auth_url) = parse_self_auth_url(&source)? else {
        return Ok(None);
    };

    let mut cookies = HostCookies::default();
    let (mut dashboard_url, mut dashboard_html) =
        account_discovery_get_follow(&client, auth_url, &mut cookies, Some(&discovery_url)).await?;
    if dashboard_url.host_str() != Some(BILLING_HOST)
        || !(dashboard_url.path() == BILLING_DASHBOARD_PATH
            || dashboard_url
                .path()
                .starts_with(&format!("{BILLING_DASHBOARD_PATH}/")))
    {
        return Err(BillingError::Protocol(
            "计费系统未进入 dashboard".to_string(),
        ));
    }

    for attempt in 0..2 {
        let account = parse_discovered_account(&dashboard_html)?;
        if !account.pass.contains("DS424:") {
            return Ok(Some(account));
        }
        if attempt == 1 {
            return Err(BillingError::Protocol(
                "dashboard 连续返回临时密码字段，请稍后重试".to_string(),
            ));
        }
        dashboard_url.query_pairs_mut().append_pair(
            "account_refresh",
            &SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .to_string(),
        );
        let refreshed =
            account_discovery_get_follow(&client, dashboard_url.clone(), &mut cookies, None)
                .await?;
        dashboard_url = refreshed.0;
        dashboard_html = refreshed.1;
    }
    Ok(None)
}

async fn get_follow(
    client: &Client,
    mut url: Url,
    cookies: &mut SessionCookies,
    referer: Option<&Url>,
    accept: &str,
    destination: &str,
) -> Result<(Url, Response), BillingError> {
    let mut current_referer = referer.map(Url::to_string);
    for _ in 0..=MAX_REDIRECTS {
        validate_same_origin(&url)?;
        let mut request = client
            .get(url.clone())
            .header(ACCEPT, accept)
            .header("Sec-Fetch-Dest", destination)
            .header(
                "Sec-Fetch-Mode",
                if destination == "document" {
                    "navigate"
                } else if destination == "empty" {
                    "cors"
                } else {
                    "no-cors"
                },
            )
            .header(
                "Sec-Fetch-Site",
                if current_referer.is_some() {
                    "same-origin"
                } else {
                    "none"
                },
            );
        if destination == "empty" {
            request = request.header("X-Requested-With", "XMLHttpRequest");
        }
        if let Some(value) = cookies.header() {
            request = request.header(COOKIE, value);
        }
        if let Some(value) = current_referer.as_deref() {
            request = request.header(REFERER, value);
        }
        let response = request.send().await.map_err(network_error)?;
        cookies.absorb(response.headers());
        if response.status().is_redirection() {
            let next = redirect_target(&url, &response)?;
            current_referer = Some(url.to_string());
            url = next;
            continue;
        }
        if !response.status().is_success() {
            return Err(BillingError::Protocol(format!(
                "服务器返回 HTTP {}",
                response.status().as_u16()
            )));
        }
        return Ok((url, response));
    }
    Err(BillingError::Protocol("重定向次数过多".to_string()))
}

async fn post_login(
    client: &Client,
    verify_url: Url,
    referer: &Url,
    cookies: &mut SessionCookies,
    checkcode: &str,
    account: &str,
    password: &str,
) -> Result<(Url, String), BillingError> {
    validate_login_action(&verify_url)?;
    let form = [
        ("checkcode", checkcode),
        ("account", account),
        ("password", password),
        ("code", ""),
    ];
    let mut request = client
        .post(verify_url.clone())
        .header(
            ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .header(ORIGIN, BILLING_ORIGIN)
        .header(REFERER, referer.as_str())
        .header("Sec-Fetch-Dest", "document")
        .header("Sec-Fetch-Mode", "navigate")
        .header("Sec-Fetch-Site", "same-origin")
        .form(&form);
    if let Some(value) = cookies.header() {
        request = request.header(COOKIE, value);
    }
    let response = request.send().await.map_err(network_error)?;
    cookies.absorb(response.headers());
    if response.status().is_redirection() {
        if matches!(response.status().as_u16(), 307 | 308) {
            return Err(BillingError::Protocol(
                "登录端点要求重复提交密码，已安全中止".to_string(),
            ));
        }
        let next = redirect_target(&verify_url, &response)?;
        let (final_url, response) = get_follow(
            client,
            next,
            cookies,
            Some(&verify_url),
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            "document",
        )
        .await?;
        let text = response.text().await.map_err(network_error)?;
        return Ok((final_url, text));
    }
    if !response.status().is_success() {
        return Err(BillingError::Protocol(format!(
            "登录端点返回 HTTP {}",
            response.status().as_u16()
        )));
    }
    let text = response.text().await.map_err(network_error)?;
    Ok((verify_url, text))
}

fn network_error(error: reqwest::Error) -> BillingError {
    BillingError::Network(redact_request_error(error))
}

fn redirect_target(current: &Url, response: &Response) -> Result<Url, BillingError> {
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| BillingError::Protocol("重定向缺少 Location".to_string()))?;
    let next = current
        .join(location)
        .map_err(|_| BillingError::Protocol("重定向地址无效".to_string()))?;
    validate_same_origin(&next)?;
    Ok(next)
}

fn same_origin_url(value: &str) -> Result<Url, BillingError> {
    let url =
        Url::parse(value).map_err(|_| BillingError::Protocol("计费系统地址无效".to_string()))?;
    validate_same_origin(&url)?;
    Ok(url)
}

fn validate_same_origin(url: &Url) -> Result<(), BillingError> {
    if url.scheme() != "https"
        || url.host_str() != Some(BILLING_HOST)
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(BillingError::Protocol(
            "服务器尝试跳转到非受信任地址".to_string(),
        ));
    }
    Ok(())
}

fn validate_login_action(url: &Url) -> Result<(), BillingError> {
    validate_same_origin(url)?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err(BillingError::Protocol(
            "登录表单包含未预期的参数".to_string(),
        ));
    }
    let path = url.path();
    if path == "/Self/login/verify" {
        return Ok(());
    }
    let Some(session) = path.strip_prefix("/Self/login/verify;jsessionid=") else {
        return Err(BillingError::Protocol(
            "登录表单提交地址不受信任".to_string(),
        ));
    };
    if session.is_empty()
        || !session
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(BillingError::Protocol("登录会话标识格式无效".to_string()));
    }
    Ok(())
}

fn login_action(html: &str, base_url: &Url) -> Result<Url, BillingError> {
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(relative) = lower[cursor..].find("<form") {
        let start = cursor + relative;
        let Some(tag_end) = tag_end(html, start) else {
            break;
        };
        let form_tag = &html[start..=tag_end];
        if attribute(form_tag, "method")
            .unwrap_or_else(|| "get".to_string())
            .eq_ignore_ascii_case("post")
        {
            let close = lower[tag_end + 1..]
                .find("</form>")
                .map(|offset| tag_end + 1 + offset)
                .unwrap_or(html.len());
            let form_html = &html[start..close];
            if ["checkcode", "account", "password", "code"]
                .iter()
                .all(|name| input_named(form_html, name))
            {
                let action = attribute(form_tag, "action")
                    .unwrap_or_else(|| "/Self/login/verify".to_string());
                let url = base_url
                    .join(&action)
                    .map_err(|_| BillingError::Protocol("登录表单提交地址无效".to_string()))?;
                validate_login_action(&url)?;
                return Ok(url);
            }
        }
        cursor = tag_end + 1;
    }
    Err(BillingError::Protocol("未找到受支持的登录表单".to_string()))
}

fn login_form_present(html: &str, base_url: &Url) -> bool {
    login_action(html, base_url).is_ok()
}

fn input_named(html: &str, wanted: &str) -> bool {
    tags(html, "input")
        .into_iter()
        .any(|tag| attribute(tag, "name").is_some_and(|name| name.eq_ignore_ascii_case(wanted)))
}

fn input_value(html: &str, wanted: &str) -> Option<String> {
    tags(html, "input").into_iter().find_map(|tag| {
        let name = attribute(tag, "name")?;
        name.eq_ignore_ascii_case(wanted)
            .then(|| attribute(tag, "value").unwrap_or_default())
    })
}

fn captcha_required(html: &str) -> Result<bool, BillingError> {
    for tag in tags(html, "div") {
        if attribute(tag, "id").as_deref() == Some("randomDiv") {
            let classes = attribute(tag, "class").unwrap_or_default();
            return Ok(!classes
                .split_whitespace()
                .any(|class| class.eq_ignore_ascii_case("hide")));
        }
    }
    Err(BillingError::Protocol(
        "登录页缺少验证码状态标记".to_string(),
    ))
}

async fn fetch_page_csrf_token(
    client: &Client,
    cookies: &mut SessionCookies,
    html: &str,
    dashboard_url: &Url,
) -> Option<String> {
    if let Some(token) = extract_ajax_csrf_token(html) {
        return Some(token);
    }
    for tag in tags(html, "script") {
        let Some(source) = attribute(tag, "src") else {
            continue;
        };
        let Ok(url) = dashboard_url.join(&source) else {
            continue;
        };
        if validate_same_origin(&url).is_err()
            || !url.path().to_ascii_lowercase().ends_with("/sharejs.js")
        {
            continue;
        }
        let Ok((_, response)) =
            get_follow(client, url, cookies, Some(dashboard_url), "*/*", "script").await
        else {
            continue;
        };
        let Ok(source) = response.text().await else {
            continue;
        };
        if let Some(token) = extract_ajax_csrf_token(&source) {
            return Some(token);
        }
    }
    None
}

fn extract_ajax_csrf_token(source: &str) -> Option<String> {
    extract_literal_token(source, "ajaxcsrftoken")
}

fn extract_literal_token(source: &str, needle: &str) -> Option<String> {
    if needle.is_empty() || !needle.is_ascii() {
        return None;
    }
    let lower = source.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(relative) = lower[cursor..].find(&needle) {
        let start = cursor + relative;
        let bytes = source.as_bytes();
        let before_ok =
            start == 0 || !bytes[start - 1].is_ascii_alphanumeric() && bytes[start - 1] != b'_';
        let mut index = start + needle.len();
        let after_ok =
            index >= bytes.len() || !bytes[index].is_ascii_alphanumeric() && bytes[index] != b'_';
        if !before_ok || !after_ok {
            cursor = index;
            continue;
        }
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if !bytes
            .get(index)
            .is_some_and(|byte| matches!(*byte, b':' | b'='))
        {
            cursor = index;
            continue;
        }
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        let quote = bytes
            .get(index)
            .copied()
            .filter(|byte| matches!(byte, b'\'' | b'"'));
        if quote.is_some() {
            index += 1;
        }
        let value_start = index;
        while let Some(byte) = bytes.get(index).copied() {
            if quote
                .map(|active| byte == active)
                .unwrap_or_else(|| byte.is_ascii_whitespace() || matches!(byte, b',' | b'}'))
            {
                break;
            }
            index += 1;
        }
        let value = &source[value_start..index];
        if !value.is_empty()
            && value.len() <= 256
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Some(value.to_string());
        }
        cursor = index.max(start + needle.len());
    }
    None
}

fn login_assets(html: &str, base_url: &Url) -> Vec<(Url, &'static str)> {
    let mut result = Vec::new();
    for (tag_name, attribute_name, destination) in
        [("script", "src", "script"), ("link", "href", "style")]
    {
        for tag in tags(html, tag_name) {
            let Some(value) = attribute(tag, attribute_name) else {
                continue;
            };
            let Ok(url) = base_url.join(&value) else {
                continue;
            };
            if validate_same_origin(&url).is_err() {
                continue;
            }
            let path = url.path().to_ascii_lowercase();
            if !path.ends_with(".js") && !path.ends_with(".css") {
                continue;
            }
            if !result.iter().any(|(existing, _)| existing == &url) {
                result.push((url, destination));
            }
            if result.len() >= 16 {
                return result;
            }
        }
    }
    result
}

fn tags<'a>(html: &'a str, name: &str) -> Vec<&'a str> {
    let lower = html.to_ascii_lowercase();
    let needle = format!("<{name}");
    let mut cursor = 0usize;
    let mut result = Vec::new();
    while let Some(relative) = lower[cursor..].find(&needle) {
        let start = cursor + relative;
        let boundary = lower.as_bytes().get(start + needle.len()).copied();
        if boundary.is_some_and(|byte| !byte.is_ascii_whitespace() && byte != b'>' && byte != b'/')
        {
            cursor = start + needle.len();
            continue;
        }
        let Some(end) = tag_end(html, start) else {
            break;
        };
        result.push(&html[start..=end]);
        cursor = end + 1;
    }
    result
}

fn tag_blocks<'a>(html: &'a str, name: &str) -> Vec<(&'a str, &'a str)> {
    let lower = html.to_ascii_lowercase();
    let open_needle = format!("<{name}");
    let close_needle = format!("</{name}>");
    let mut cursor = 0usize;
    let mut result = Vec::new();
    while let Some(relative) = lower[cursor..].find(&open_needle) {
        let start = cursor + relative;
        let boundary = lower.as_bytes().get(start + open_needle.len()).copied();
        if boundary.is_some_and(|byte| !byte.is_ascii_whitespace() && byte != b'>' && byte != b'/')
        {
            cursor = start + open_needle.len();
            continue;
        }
        let Some(open_end) = tag_end(html, start) else {
            break;
        };
        let Some(close_relative) = lower[open_end + 1..].find(&close_needle) else {
            cursor = open_end + 1;
            continue;
        };
        let close_start = open_end + 1 + close_relative;
        result.push((&html[start..=open_end], &html[open_end + 1..close_start]));
        cursor = close_start + close_needle.len();
    }
    result
}

fn tag_end(html: &str, start: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut quote = None;
    for (index, byte) in bytes.iter().copied().enumerate().skip(start) {
        if let Some(active) = quote {
            if byte == active {
                quote = None;
            }
        } else if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
        } else if byte == b'>' {
            return Some(index);
        }
    }
    None
}

fn attribute(tag: &str, wanted: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let bytes = tag.as_bytes();
    let wanted = wanted.as_bytes();
    let mut cursor = 0usize;
    while cursor + wanted.len() <= bytes.len() {
        let relative = lower[cursor..].find(std::str::from_utf8(wanted).ok()?)?;
        let start = cursor + relative;
        let before_ok =
            start == 0 || bytes[start - 1].is_ascii_whitespace() || bytes[start - 1] == b'<';
        let after_index = start + wanted.len();
        let after_ok = after_index >= bytes.len()
            || bytes[after_index].is_ascii_whitespace()
            || matches!(bytes[after_index], b'=' | b'>' | b'/');
        if !before_ok || !after_ok {
            cursor = after_index;
            continue;
        }
        let mut index = after_index;
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if bytes.get(index) != Some(&b'=') {
            return Some(String::new());
        }
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        let quote = bytes
            .get(index)
            .copied()
            .filter(|byte| matches!(byte, b'\'' | b'"'));
        if quote.is_some() {
            index += 1;
        }
        let value_start = index;
        while let Some(byte) = bytes.get(index).copied() {
            if quote
                .map(|active| byte == active)
                .unwrap_or_else(|| byte.is_ascii_whitespace() || byte == b'>')
            {
                break;
            }
            index += 1;
        }
        return Some(decode_html_entities(&tag[value_start..index]));
    }
    None
}

async fn parse_dashboard_bounded(
    html: &str,
    account: &str,
) -> Result<BillingSnapshot, BillingError> {
    let html = html.to_string();
    let account = account.to_string();
    match tokio::time::timeout(
        Duration::from_secs(3),
        tokio::task::spawn_blocking(move || parse_dashboard(&html, &account)),
    )
    .await
    {
        Ok(Ok(snapshot)) => Ok(snapshot),
        Ok(Err(error)) => Err(BillingError::Protocol(format!(
            "账户概览解析任务异常：{error}"
        ))),
        Err(_) => Err(BillingError::Protocol(
            "账户概览解析超过 3 秒，已停止本次请求".to_string(),
        )),
    }
}

fn parse_dashboard(html: &str, account: &str) -> BillingSnapshot {
    let user = embedded_user_json(html)
        .and_then(|json| serde_json::from_str::<DashboardUser>(json).ok())
        .unwrap_or_default();
    let mut warnings = Vec::new();
    let balance = extract_dl_metric(html, "账户余额")
        .or_else(|| {
            user.left_money
                .map(|value| format!("{} 元", compact_decimal(value, 4)))
        })
        .unwrap_or_else(|| {
            warnings.push("控制台没有返回账户余额".to_string());
            "--".to_string()
        });
    let remaining_flow = extract_dl_metric(html, "可用流量")
        .or_else(|| extract_dl_metric(html, "剩余流量"))
        .map(|raw| {
            format_remaining_flow(&raw).unwrap_or_else(|_| {
                warnings.push("控制台返回了无法识别的剩余流量格式".to_string());
                normalize_text(&raw)
            })
        })
        .unwrap_or_else(|| {
            warnings.push("控制台没有返回剩余流量".to_string());
            "--".to_string()
        });
    let status_reason = user.stop_reason.filter(|value| !value.trim().is_empty());
    let status = if status_reason.is_some() || user.use_flag == Some(0) {
        Some("停机".to_string())
    } else if user.use_flag == Some(1) {
        Some("正常".to_string())
    } else {
        None
    };
    let service = user.service_default.unwrap_or_default();

    BillingSnapshot {
        account: account.to_string(),
        balance: normalize_text(&balance),
        remaining_flow,
        used_flow: extract_dl_metric(html, "已用流量").map(|value| normalize_text(&value)),
        status,
        status_reason,
        package: service
            .default_name
            .filter(|value| !value.trim().is_empty()),
        package_detail: service.extend.filter(|value| !value.trim().is_empty()),
        billing_cycle: extract_billing_cycle(html),
        updated_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        login_history: Vec::new(),
        online_sessions: Vec::new(),
        offline_tip: None,
        mauth_enabled: None,
        warnings,
    }
}

fn parse_json_payload(text: &str, label: &str) -> Result<serde_json::Value, BillingError> {
    let trimmed = text.trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        return Ok(serde_json::Value::Null);
    }
    let value: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|_| BillingError::Protocol(format!("{label}不是有效 JSON")))?;
    if let Some(nested) = value.as_str() {
        let nested = nested.trim_start_matches('\u{feff}').trim();
        if nested.starts_with('{') || nested.starts_with('[') || nested == "null" {
            return serde_json::from_str(nested)
                .map_err(|_| BillingError::Protocol(format!("{label}包含无效的嵌套 JSON")));
        }
    }
    Ok(value)
}

fn response_rows<'a>(
    value: &'a serde_json::Value,
    label: &str,
) -> Result<&'a Vec<serde_json::Value>, BillingError> {
    if let Some(rows) = value.as_array() {
        return Ok(rows);
    }
    if let Some(rows) = value
        .as_object()
        .and_then(|object| object.get("rows").or_else(|| object.get("data")))
        .and_then(serde_json::Value::as_array)
    {
        return Ok(rows);
    }
    Err(BillingError::Protocol(format!("{label}不是记录数组")))
}

fn parse_login_history(text: &str) -> Result<Vec<BillingLoginRecord>, BillingError> {
    let value = parse_json_payload(text, "最近上网记录")?;
    if value.is_null() {
        return Ok(Vec::new());
    }
    let rows = response_rows(&value, "最近上网记录")?;
    if rows.len() > 5_000 {
        return Err(BillingError::Protocol(
            "最近上网记录数量超过安全上限".to_string(),
        ));
    }

    rows.iter()
        .map(|row| {
            let cells = row
                .as_array()
                .ok_or_else(|| BillingError::Protocol("最近上网记录包含非数组条目".to_string()))?;
            if cells.len() < 9 {
                return Err(BillingError::Protocol(
                    "最近上网记录字段数量不足".to_string(),
                ));
            }
            Ok(BillingLoginRecord {
                login_at: format_time_value(&cells[0]),
                logout_at: format_time_value(&cells[1]),
                ip: json_display(&cells[2]),
                ipv6: json_display(&cells[3]),
                mac: format_mac(&json_display(&cells[4])),
                duration_minutes: json_display(&cells[5]),
                used_flow_mb: json_display(&cells[6]),
                billing_mode: match value_as_i64(&cells[7]) {
                    Some(1) => "时长".to_string(),
                    Some(2) => "流量".to_string(),
                    Some(3) => "包月".to_string(),
                    _ => json_display(&cells[7]),
                },
                amount: json_display(&cells[8]),
            })
        })
        .collect()
}

fn parse_online_sessions(text: &str) -> Result<Vec<BillingOnlineSession>, BillingError> {
    let value = parse_json_payload(text, "在线会话")?;
    if value.is_null() {
        return Ok(Vec::new());
    }
    let rows = response_rows(&value, "在线会话")?;
    if rows.len() > 500 {
        return Err(BillingError::Protocol(
            "在线会话数量超过安全上限".to_string(),
        ));
    }

    rows.iter()
        .map(|row| {
            let object = row
                .as_object()
                .ok_or_else(|| BillingError::Protocol("在线会话包含非对象条目".to_string()))?;
            let session_id = object
                .get("sessionId")
                .map(json_display)
                .filter(|value| value != "--" && !value.is_empty())
                .ok_or_else(|| BillingError::Protocol("在线会话缺少标识".to_string()))?;
            let duration_minutes = object
                .get("useTime")
                .and_then(value_as_f64)
                .map(|value| format!("{:.0}", value / 60.0))
                .unwrap_or_else(|| "--".to_string());
            let used_flow_mb = match (
                object.get("downFlow").and_then(value_as_f64),
                object.get("upFlow").and_then(value_as_f64),
            ) {
                (Some(down), Some(up)) => format!("{:.3}", (down + up) / 1024.0),
                _ => "--".to_string(),
            };
            Ok(BillingOnlineSession {
                login_at: object
                    .get("loginTime")
                    .map(format_time_value)
                    .unwrap_or_else(|| "--".to_string()),
                ip: object
                    .get("ip")
                    .map(json_display)
                    .unwrap_or_else(|| "--".to_string()),
                ipv6: object
                    .get("ipv6")
                    .map(json_display)
                    .unwrap_or_else(|| "--".to_string()),
                mac: object
                    .get("mac")
                    .and_then(serde_json::Value::as_str)
                    .map(normalize_text)
                    .map(|value| format_mac(&value))
                    .unwrap_or_default(),
                duration_minutes,
                used_flow_mb,
                session_id,
            })
        })
        .collect()
}

fn parse_offline_tip(text: &str) -> Result<Option<String>, BillingError> {
    let value = normalize_text(text);
    if value.chars().count() > 500 {
        return Err(BillingError::Protocol(
            "注销提示长度超过安全上限".to_string(),
        ));
    }
    Ok((!value.is_empty()).then_some(value))
}

fn parse_mauth_state(text: &str) -> Result<bool, BillingError> {
    let trimmed = text.trim_start_matches('\u{feff}').trim();
    let html = serde_json::from_str::<String>(trimmed).unwrap_or_else(|_| trimmed.to_string());
    let action = html_text(&html);
    if action.contains("开启") {
        Ok(true)
    } else if action.contains("关闭") {
        Ok(false)
    } else {
        Err(BillingError::Protocol(
            "无感认证状态内容无法识别".to_string(),
        ))
    }
}

fn parse_billing_table(
    text: &str,
    array_fields: &[&str],
    label: &str,
) -> Result<BillingTable, BillingError> {
    let value = parse_json_payload(text, label)?;
    if value.is_null() {
        return Ok(BillingTable::default());
    }
    let (rows, total, summary) = if let Some(rows) = value.as_array() {
        (rows, rows.len() as u64, BTreeMap::new())
    } else if let Some(object) = value.as_object() {
        let rows = object
            .get("rows")
            .or_else(|| object.get("data"))
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| BillingError::Protocol(format!("{label}缺少 rows 数组")))?;
        let total = object
            .get("total")
            .and_then(value_as_u64)
            .unwrap_or(rows.len() as u64);
        let summary = object
            .get("summary")
            .and_then(serde_json::Value::as_object)
            .map(|values| {
                values
                    .iter()
                    .filter_map(|(name, value)| {
                        billing_summary_label(label, name)
                            .map(|display_name| (display_name.to_string(), json_display(value)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        (rows, total, summary)
    } else {
        return Err(BillingError::Protocol(format!(
            "{label}既不是数组也不是分页对象"
        )));
    };
    if rows.len() > 5_000 || total > 1_000_000 {
        return Err(BillingError::Protocol(format!("{label}数量超过安全上限")));
    }

    let rows = rows
        .iter()
        .map(|row| {
            let mut mapped = BTreeMap::new();
            if let Some(values) = row.as_array() {
                for (key, value) in array_fields.iter().zip(values) {
                    mapped.insert((*key).to_string(), format_table_cell(key, value));
                }
            } else if let Some(values) = row.as_object() {
                for (key, value) in values {
                    if value.is_array() || value.is_object() {
                        continue;
                    }
                    let Some(allowed) = array_fields
                        .iter()
                        .find(|allowed| allowed.eq_ignore_ascii_case(key))
                    else {
                        continue;
                    };
                    mapped.insert((*allowed).to_string(), format_table_cell(allowed, value));
                }
            } else {
                return Err(BillingError::Protocol(format!("{label}包含无法识别的记录")));
            }
            if mapped.is_empty() && !row.as_array().is_some_and(Vec::is_empty) {
                return Err(BillingError::Protocol(format!(
                    "{label}记录不包含允许展示的字段"
                )));
            }
            Ok(mapped)
        })
        .collect::<Result<Vec<_>, BillingError>>()?;
    let mut table = BillingTable {
        total,
        rows,
        summary,
    };
    if label == "上网记录" {
        sanitize_usage_table(&mut table);
    }
    Ok(table)
}

fn sanitize_usage_table(table: &mut BillingTable) {
    for row in &mut table.rows {
        let mac = row.get("macAddress").map(String::as_str).unwrap_or("--");
        let nas_ip = row.get("nasIp").map(String::as_str).unwrap_or("--");
        let nas_port = row.get("nasPort").map(String::as_str).unwrap_or("--");
        let access_method = if normalize_mac_for_action(mac) == "000000000000"
            && nas_ip == "0.0.0.0"
            && nas_port == "0"
        {
            "lgn（有线）"
        } else if nas_ip == "10.21.251.3" {
            "bjut_wifi"
        } else if nas_ip == "10.21.221.97" {
            "bjut-sushe"
        } else {
            "未知"
        };
        for field in [
            "internetUpFlow",
            "internetDownFlow",
            "chinanetUpFlow",
            "chinanetDownFlow",
            "nasIp",
            "nasPort",
        ] {
            row.remove(field);
        }
        row.insert("accessMethod".to_string(), access_method.to_string());
    }
}

fn billing_summary_label(table_label: &str, raw: &str) -> Option<&'static str> {
    Some(match (table_label, raw.to_ascii_uppercase().as_str()) {
        ("上网记录", "COSTMONEY") => "计费金额(元)",
        ("上网记录", "COU") => "记录数",
        ("上网记录", "FLOW") => "使用流量(MB)",
        ("上网记录", "TIME") => "使用时长(分钟)",
        ("历史账单", "USEBASEMONEY") => "基本月租(元)",
        ("历史账单", "USEDMONEY") => "时长/流量计费(元)",
        ("历史账单", "USETIME") => "使用时长(分钟)",
        ("历史账单", "USEFLOW") => "使用流量(MB)",
        ("充值明细", "MONEY") => "金额(元)",
        _ => return None,
    })
}

fn format_table_cell(key: &str, value: &serde_json::Value) -> String {
    let lower = key.to_ascii_lowercase();
    let is_timestamp = lower.contains("date")
        || lower.ends_with("at")
        || matches!(lower.as_str(), "logintime" | "logouttime" | "lastlogintime");
    if is_timestamp && value_as_i64(value).is_some_and(|value| value > 10_000_000_000) {
        return format_time_value(value);
    }
    let displayed = json_display(value);
    if lower.contains("mac") && displayed != "--" {
        format_mac(&displayed)
    } else {
        displayed
    }
}

fn value_as_u64(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
}

fn parse_service_state(
    dashboard_html: &str,
    stop_html: &str,
    reopen_html: &str,
    package_html: &str,
    protect_html: &str,
    active_package_reservation: Option<&ActivePackageReservation>,
) -> BillingServiceState {
    let mut user = dashboard_user(dashboard_html);
    let protect_user = dashboard_user(protect_html);
    user.installment_flag = user.installment_flag.or(protect_user.installment_flag);
    user.use_money = user.use_money.or(protect_user.use_money);
    user.left_money = user.left_money.or(protect_user.left_money);
    let service = user.service_default.clone().unwrap_or_default();
    let current_package_id = service.id.map(|id| id.to_string());
    let current_package = service.default_name.clone();
    let package_detail = service.extend.clone();
    let scheduled_package = active_package_reservation
        .map(|reservation| reservation.scheduled_package.clone())
        .or_else(|| scheduled_package_name(package_html));
    let reservation_current_package =
        active_package_reservation.and_then(|reservation| reservation.current_package.as_deref());
    let package_scheduled = (active_package_reservation.is_some()
        || has_package_cancel_control(package_html))
        && package_reservation_is_distinct(
            current_package.as_deref().or(reservation_current_package),
            scheduled_package.as_deref(),
        );
    let mut package_options = parse_package_options(package_html);
    if let (Some(id), Some(name)) = (current_package_id.as_ref(), current_package.as_ref()) {
        if !package_options.iter().any(|option| option.id == *id) {
            package_options.insert(
                0,
                BillingPackageOption {
                    id: id.clone(),
                    name: name.clone(),
                    description: package_detail.clone().unwrap_or_default(),
                },
            );
        }
    }
    let scheduled_package_id = if package_scheduled {
        let scheduled_name = normalize_text(scheduled_package.as_deref().unwrap_or_default());
        package_options
            .iter()
            .find(|option| normalize_text(&option.name).eq_ignore_ascii_case(&scheduled_name))
            .map(|option| option.id.clone())
    } else {
        None
    };
    let stopped = user.use_flag == Some(0) || user.stop_reason.is_some();
    let can_stop_now = user.use_flag == Some(1) && has_enabled_element_id(stop_html, "stopNow");
    let can_reopen_now = stopped && has_enabled_element_id(reopen_html, "reOpenNow");
    BillingServiceState {
        account_status: user.use_flag.map(|flag| {
            if flag == 1 {
                "正常".to_string()
            } else {
                "停机".to_string()
            }
        }),
        status_reason: user.stop_reason.clone(),
        current_package_id,
        current_package,
        package_detail,
        next_settlement_date: date_near_label(stop_html, "下次结算日期"),
        can_stop_now,
        can_reopen_now,
        package_scheduled,
        scheduled_package_id,
        scheduled_package,
        consume_limit: user.installment_flag.map(|value| {
            if (value - 999_999.0).abs() < f64::EPSILON {
                "不限制".to_string()
            } else {
                format!("{} 元", compact_decimal(value, 3))
            }
        }),
        current_cycle_spend: user
            .use_money
            .map(|value| format!("{} 元", compact_decimal(value, 3))),
        balance: user
            .left_money
            .map(|value| format!("{} 元", compact_decimal(value, 4))),
        package_options,
    }
}

fn parse_active_package_reservation(
    text: &str,
) -> Result<Option<ActivePackageReservation>, BillingError> {
    let table = parse_billing_table(text, PACKAGE_LOG_FIELDS, "预约套餐记录")?;
    let Some(latest) = table.rows.first() else {
        return Ok(None);
    };
    if latest.get("fldstate").map(String::as_str) != Some("1") {
        return Ok(None);
    }
    let scheduled_package = latest
        .get("flddefaultname2")
        .map(|value| normalize_text(value))
        .filter(|value| !value.is_empty() && value != "--")
        .ok_or_else(|| BillingError::Protocol("有效套餐预约缺少目标套餐".to_string()))?;
    let current_package = latest
        .get("flddefaultname1")
        .map(|value| normalize_text(value))
        .filter(|value| !value.is_empty() && value != "--");
    Ok(Some(ActivePackageReservation {
        current_package,
        scheduled_package,
    }))
}

fn package_reservation_is_distinct(current: Option<&str>, scheduled: Option<&str>) -> bool {
    let Some(scheduled) = scheduled
        .map(normalize_text)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    current
        .map(normalize_text)
        .filter(|value| !value.is_empty())
        .is_none_or(|current| !current.eq_ignore_ascii_case(&scheduled))
}

fn date_near_label(html: &str, label: &str) -> Option<String> {
    let start = html.find(label)?;
    dates_in(char_boundary_window(html, start, 800))
        .into_iter()
        .next()
}

fn has_tag_attribute(html: &str, name: &str, value: &str) -> bool {
    ["a", "button", "input"]
        .into_iter()
        .flat_map(|tag_name| tags(html, tag_name))
        .any(|tag| attribute(tag, name).as_deref() == Some(value))
}

fn has_package_cancel_control(html: &str) -> bool {
    has_tag_attribute(html, "data-toggle", "undoPackage")
        || ["a", "button", "input"]
            .into_iter()
            .flat_map(|tag_name| tags(html, tag_name))
            .any(|tag| {
                ["href", "action", "onclick"]
                    .into_iter()
                    .filter_map(|name| attribute(tag, name))
                    .any(|value| value.to_ascii_lowercase().contains("undopackage"))
            })
}

fn scheduled_package_name(html: &str) -> Option<String> {
    let tag = ["a", "button", "input"]
        .into_iter()
        .flat_map(|tag_name| tags(html, tag_name))
        .find(|tag| attribute(tag, "data-toggle").as_deref() == Some("undoPackage"))?;
    for name in ["data-package-name", "data-name", "title"] {
        if let Some(value) = attribute(tag, name).map(|value| normalize_text(&value)) {
            if !value.is_empty() && !value.contains("取消") && value.chars().count() <= 80 {
                return Some(value);
            }
        }
    }

    let tag_start = html.find(tag)?;
    let mut start = tag_start.saturating_sub(1_200);
    while start < tag_start && !html.is_char_boundary(start) {
        start += 1;
    }
    let end = tag_start.saturating_add(1_200).min(html.len());
    let mut end = end;
    while end > tag_start && !html.is_char_boundary(end) {
        end -= 1;
    }
    let text = html_text(&html[start..end]);
    for marker in ["下一周期套餐：", "预约套餐：", "新套餐："] {
        let Some((_, tail)) = text.rsplit_once(marker) else {
            continue;
        };
        let name = tail
            .split(['；', '。', '，'])
            .next()
            .unwrap_or_default()
            .trim()
            .trim_end_matches("取消套餐预约")
            .trim();
        if !name.is_empty() && name.chars().count() <= 80 {
            return Some(name.to_string());
        }
    }
    None
}

fn has_enabled_element_id(html: &str, id: &str) -> bool {
    ["button", "input", "a"]
        .into_iter()
        .flat_map(|tag_name| tags(html, tag_name))
        .any(|tag| {
            attribute(tag, "id").as_deref() == Some(id) && attribute(tag, "disabled").is_none()
        })
}

fn parse_package_options(html: &str) -> Vec<BillingPackageOption> {
    tag_blocks(html, "a")
        .into_iter()
        .filter_map(|(tag, body)| {
            let classes = attribute(tag, "class").unwrap_or_default();
            if !classes.split_whitespace().any(|class| class == "pick-card") {
                return None;
            }
            let id = attribute(tag, "data-package")?;
            if id.is_empty()
                || id.len() > 32
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                return None;
            }
            let text = html_text(body);
            let (name, description) = text.split_once("描述：").unwrap_or((&text, ""));
            let name = name
                .trim()
                .strip_prefix("套餐：")
                .unwrap_or(name.trim())
                .trim()
                .to_string();
            if name.is_empty() {
                return None;
            }
            Some(BillingPackageOption {
                id,
                name,
                description: description.trim().to_string(),
            })
        })
        .take(100)
        .collect()
}

fn parse_password_policy(html: &str) -> BillingPasswordPolicy {
    let mut policy = BillingPasswordPolicy {
        min_length: 8,
        max_length: 16,
        require_uppercase: false,
        require_lowercase: false,
        require_digit: false,
        require_special: false,
    };
    let Some(marker) = html.find("passwordPolicy") else {
        return policy;
    };
    let Some(relative_start) = html[marker..].find('{') else {
        return policy;
    };
    let start = marker + relative_start;
    let Some(end) = balanced_json_object(html, start) else {
        return policy;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&html[start..=end]) else {
        return policy;
    };
    let Some(object) = value.as_object() else {
        return policy;
    };
    policy.min_length = object
        .get("FLDMINLENGTH")
        .and_then(value_as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| (1..=128).contains(value))
        .unwrap_or(policy.min_length);
    policy.require_uppercase = object.get("FLDREQUIREUPPER").and_then(value_as_i64) == Some(1);
    policy.require_lowercase = object.get("FLDREQUIRELOWER").and_then(value_as_i64) == Some(1);
    policy.require_digit = object.get("FLDREQUIREDIGIT").and_then(value_as_i64) == Some(1);
    policy.require_special = object.get("FLDREQUIRESPECIAL").and_then(value_as_i64) == Some(1);
    policy
}

fn parse_security_questions(html: &str) -> Vec<BillingSecurityQuestion> {
    let select_body = tag_blocks(html, "select")
        .into_iter()
        .find(|(tag, _)| attribute(tag, "id").as_deref() == Some("question1"))
        .map(|(_, body)| body)
        .unwrap_or("");
    tag_blocks(select_body, "option")
        .into_iter()
        .filter_map(|(tag, body)| {
            let id = attribute(tag, "value")?;
            let text = html_text(body);
            (!id.is_empty() && id != "0" && !text.is_empty())
                .then_some(BillingSecurityQuestion { id, text })
        })
        .take(100)
        .collect()
}

fn value_as_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.trim().parse::<i64>().ok())
}

fn value_as_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

fn json_display(value: &serde_json::Value) -> String {
    let text = match value {
        serde_json::Value::Null => return "--".to_string(),
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        _ => return "--".to_string(),
    };
    let normalized = normalize_text(&text);
    if normalized.is_empty() {
        "--".to_string()
    } else {
        normalized
    }
}

fn format_time_value(value: &serde_json::Value) -> String {
    let timestamp = value_as_i64(value);
    if let Some(timestamp) = timestamp {
        if timestamp <= 0 {
            return "--".to_string();
        }
        if let Some(datetime) = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp) {
            return datetime
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string();
        }
    }
    json_display(value)
}

fn format_mac(value: &str) -> String {
    let normalized = normalize_mac_for_action(value);
    if normalized.len() != 12 {
        return value.to_string();
    }
    normalized
        .as_bytes()
        .chunks(2)
        .filter_map(|chunk| std::str::from_utf8(chunk).ok())
        .collect::<Vec<_>>()
        .join("-")
}

fn embedded_user_json(html: &str) -> Option<&str> {
    let marker = html.find("window.user")?;
    let mut cursor = marker;
    while let Some(relative) = html[cursor..].find("})") {
        let closure_end = cursor + relative + 2;
        let bytes = html.as_bytes();
        let mut index = closure_end;
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if bytes.get(index) != Some(&b'(') {
            cursor = closure_end;
            continue;
        }
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if bytes.get(index) != Some(&b'{') {
            cursor = closure_end;
            continue;
        }
        let end = balanced_json_object(html, index)?;
        return Some(&html[index..=end]);
    }
    None
}

fn balanced_json_object(source: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in source.as_bytes().iter().copied().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn extract_dl_metric(html: &str, label: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(relative) = lower[cursor..].find("<dl") {
        let start = cursor + relative;
        let end = lower[start..]
            .find("</dl>")
            .map(|offset| start + offset + 5)?;
        let block = &html[start..end];
        if html_text(block).contains(label) {
            let block_lower = block.to_ascii_lowercase();
            let dt_start = block_lower.find("<dt")?;
            let open_end = tag_end(block, dt_start)?;
            let dt_end = block_lower[open_end + 1..]
                .find("</dt>")
                .map(|offset| open_end + 1 + offset)?;
            return Some(html_text(&block[open_end + 1..dt_end]));
        }
        cursor = end;
    }
    None
}

fn extract_billing_cycle(html: &str) -> Option<String> {
    let label = html.find("计费周期")?;
    let dates = dates_in(char_boundary_window(html, label, 1600));
    (dates.len() >= 2).then(|| format!("{} 至 {}", dates[0], dates[1]))
}

fn char_boundary_window(source: &str, start: usize, max_bytes: usize) -> &str {
    if start > source.len() || !source.is_char_boundary(start) {
        return "";
    }
    let mut end = start.saturating_add(max_bytes).min(source.len());
    while end > start && !source.is_char_boundary(end) {
        end -= 1;
    }
    &source[start..end]
}

fn dates_in(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut result = Vec::new();
    for index in 0..bytes.len().saturating_sub(9) {
        let candidate = &bytes[index..index + 10];
        if candidate[0..4].iter().all(u8::is_ascii_digit)
            && candidate[4] == b'-'
            && candidate[5..7].iter().all(u8::is_ascii_digit)
            && candidate[7] == b'-'
            && candidate[8..10].iter().all(u8::is_ascii_digit)
        {
            result.push(String::from_utf8_lossy(candidate).into_owned());
            if result.len() == 2 {
                break;
            }
        }
    }
    result
}

fn format_remaining_flow(raw: &str) -> Result<String, BillingError> {
    let normalized = normalize_text(raw);
    if normalized.contains("无限") || normalized.contains("不限") {
        return Ok("无限".to_string());
    }
    let value = first_number(&normalized)
        .ok_or_else(|| BillingError::Protocol("可用流量数值无法解析".to_string()))?;
    if value < 0.0 {
        return Ok("无限".to_string());
    }
    let unit = normalized.to_ascii_uppercase();
    let gigabytes = if unit.contains("TB") {
        value * 1024.0
    } else if unit.contains("GB") {
        value
    } else if unit.contains("KB") {
        value / (1024.0 * 1024.0)
    } else {
        value / 1024.0
    };
    Ok(format!("{gigabytes:.2} GB"))
}

fn first_number(value: &str) -> Option<f64> {
    let mut number = String::new();
    let mut started = false;
    for character in value.chars() {
        if character.is_ascii_digit() || character == '.' || (!started && character == '-') {
            number.push(character);
            started = true;
        } else if started {
            break;
        }
    }
    number.parse().ok()
}

fn compact_decimal(value: f64, precision: usize) -> String {
    let formatted = format!("{value:.precision$}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

fn html_text(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'<' {
            if let Some(end) = tag_end(source, index) {
                result.push(' ');
                index = end + 1;
                continue;
            }
        }
        let Some(character) = source[index..].chars().next() else {
            break;
        };
        result.push(character);
        index += character.len_utf8();
    }
    normalize_text(&decode_html_entities(&result))
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_html_entities(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_signed_self_service_account_discovery_url() {
        let source = r#"dr1004({"result":1,"msg":"ok","self_auth_url":"https:\/\/jfself.bjut.edu.cn\/Self\/login\/eportalLogin?account=25000000&timestamp=1780981775208&sign=abcdef"});"#;
        let url = parse_self_auth_url(source).unwrap().unwrap();
        assert_eq!(url.host_str(), Some(BILLING_HOST));
        assert_eq!(url.path(), BILLING_EPORTAL_LOGIN_PATH);

        let unavailable = r#"dr1004({"result":0,"msg":"not connected"});"#;
        assert!(parse_self_auth_url(unavailable).unwrap().is_none());
        let attacker = r#"dr1004({"result":1,"self_auth_url":"https:\/\/example.com\/Self\/login\/eportalLogin?account=1&timestamp=2&sign=3"});"#;
        assert!(parse_self_auth_url(attacker).is_err());
    }

    #[test]
    fn extracts_dashboard_credentials_without_normalizing_password() {
        let html = r#"
          <script>(function (user) { window.user = user || {}; })({"userName":"25000000","userPassword":":Pass!word","leftMoney":3.12});</script>
        "#;
        let account = parse_discovered_account(html).unwrap();
        assert_eq!(account.user, "25000000");
        assert_eq!(account.pass, ":Pass!word");

        let temporary = r#"
          <script>(function (user) { window.user = user || {}; })({"userName":"25000000","userPassword":"DS424:temporary"});</script>
        "#;
        assert!(parse_discovered_account(temporary)
            .unwrap()
            .pass
            .contains("DS424:"));
    }

    #[test]
    fn parses_hidden_checkcode_and_session_bound_action() {
        let html = r#"
            <form action="/Self/login/verify;jsessionid=abc-123" method="post">
              <input value="321" name="checkcode"><input name="account">
              <input name="password"><input name="code">
            </form>
            <div class="form-group hide" id="randomDiv"></div>
        "#;
        let base = Url::parse(BILLING_LOGIN_URL).unwrap();
        assert_eq!(input_value(html, "checkcode").as_deref(), Some("321"));
        assert!(!captcha_required(html).unwrap());
        assert_eq!(
            login_action(html, &base).unwrap().path(),
            "/Self/login/verify;jsessionid=abc-123"
        );
    }

    #[test]
    fn rejects_cross_origin_login_actions() {
        let base = Url::parse(BILLING_LOGIN_URL).unwrap();
        for action in [
            "https://example.com/collect",
            "https://jfself.bjut.edu.cn:444/Self/login/verify",
            "https://user:pass@jfself.bjut.edu.cn/Self/login/verify",
        ] {
            let html = format!(
                r#"<form action="{action}" method="post">
                    <input name="checkcode"><input name="account">
                    <input name="password"><input name="code">
                </form>"#
            );
            assert!(login_action(&html, &base).is_err());
        }
    }

    #[test]
    fn detects_visible_captcha_before_credentials_are_submitted() {
        assert!(captcha_required(r#"<div id="randomDiv" class="form-group"></div>"#).unwrap());
    }

    #[test]
    fn parses_dashboard_without_deserializing_private_fields() {
        let html = r#"
            <script>(function (user) { window.user = user || {}; })({
              "leftMoney":12.3456,"useFlag":1,
              "userRealName":"private","userPassword":"private",
              "serviceDefault":{"defaultName":"测试套餐","extend":"套餐说明"}
            });</script>
            <dl><dt>1024 <small>MB</small></dt><dd>已用流量</dd></dl>
            <dl><dt>3122 <small>MB</small></dt><dd>可用流量</dd></dl>
            <dl><dt>12.3456 <small>元</small></dt><dd>账户余额</dd></dl>
            <label>计费周期：</label><span>2026-07-01</span> 至 <span>2026-07-31</span>
        "#;
        let snapshot = parse_dashboard(html, "synthetic-account");
        assert_eq!(snapshot.account, "synthetic-account");
        assert_eq!(snapshot.balance, "12.3456 元");
        assert_eq!(snapshot.remaining_flow, "3.05 GB");
        assert_eq!(snapshot.used_flow.as_deref(), Some("1024 MB"));
        assert_eq!(snapshot.status.as_deref(), Some("正常"));
        assert_eq!(snapshot.package.as_deref(), Some("测试套餐"));
        assert_eq!(
            snapshot.billing_cycle.as_deref(),
            Some("2026-07-01 至 2026-07-31")
        );
    }

    #[test]
    fn fixed_byte_windows_end_on_utf8_character_boundaries() {
        let cycle_prefix = "计费周期：2026-07-01 至 2026-07-31 ";
        let cycle_html = format!(
            "{cycle_prefix}{}单",
            "x".repeat(1599usize.saturating_sub(cycle_prefix.len()))
        );
        assert_eq!(
            extract_billing_cycle(&cycle_html).as_deref(),
            Some("2026-07-01 至 2026-07-31")
        );

        let settlement_prefix = "下次结算日期：2026-08-01 ";
        let settlement_html = format!(
            "{settlement_prefix}{}单",
            "x".repeat(799usize.saturating_sub(settlement_prefix.len()))
        );
        assert_eq!(
            date_near_label(&settlement_html, "下次结算日期").as_deref(),
            Some("2026-08-01")
        );
    }

    #[test]
    fn treats_negative_available_flow_as_unlimited() {
        assert_eq!(format_remaining_flow("-1 MB").unwrap(), "无限");
        assert_eq!(format_remaining_flow("不限").unwrap(), "无限");
        assert_eq!(format_remaining_flow("2 GB").unwrap(), "2.00 GB");
    }

    #[test]
    fn keeps_partial_dashboard_data_when_optional_metrics_are_missing() {
        let snapshot = parse_dashboard(
            r#"<script>(function(user){window.user=user;})({"leftMoney":3.5,"useFlag":1});</script>"#,
            "synthetic-account",
        );
        assert_eq!(snapshot.account, "synthetic-account");
        assert_eq!(snapshot.balance, "3.5 元");
        assert_eq!(snapshot.remaining_flow, "--");
        assert_eq!(snapshot.status.as_deref(), Some("正常"));
        assert_eq!(snapshot.warnings.len(), 1);
    }

    #[test]
    fn cookie_jar_keeps_only_cookie_pairs() {
        let mut headers = HeaderMap::new();
        headers.append(
            SET_COOKIE,
            "JSESSIONID=abc123; Path=/Self; Secure; HttpOnly"
                .parse()
                .unwrap(),
        );
        headers.append(SET_COOKIE, "theme=blue; Path=/".parse().unwrap());
        let mut cookies = SessionCookies::default();
        cookies.absorb(&headers);
        assert_eq!(
            cookies.header().as_deref(),
            Some("JSESSIONID=abc123; theme=blue")
        );
    }

    #[test]
    fn parses_positional_login_history_without_private_extra_columns() {
        let records = parse_login_history(
            r#"[[1721091600000,1721095200000,"10.0.0.1","2001:db8::1","AABBCCDDEEFF",60,128.5,3,0,"ignored"]]"#,
        )
        .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].ip, "10.0.0.1");
        assert_eq!(records[0].mac, "AA-BB-CC-DD-EE-FF");
        assert_eq!(records[0].billing_mode, "包月");
        assert!(!records[0].login_at.is_empty());
    }

    #[test]
    fn parses_online_session_units_and_identifier() {
        let records = parse_online_sessions(
            r#"[{"loginTime":1721091600000,"ip":"10.0.0.1","ipv6":"", "mac":"AABBCCDDEEFF","useTime":121,"downFlow":1024,"upFlow":"512","sessionId":"session-1"}]"#,
        )
        .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].duration_minutes, "2");
        assert_eq!(records[0].used_flow_mb, "1.500");
        assert_eq!(records[0].session_id, "session-1");
    }

    #[test]
    fn derives_mauth_current_state_from_server_action_label() {
        assert!(parse_mauth_state(r#""<a data-toggle='oprateMauth'>开启</a>""#).unwrap());
        assert!(!parse_mauth_state(r#""<a data-toggle='oprateMauth'>关闭</a>""#).unwrap());
        assert!(parse_mauth_state(r#""unknown""#).is_err());
    }

    #[test]
    fn rejects_untrusted_session_action_parameters() {
        assert!(validate_session_action("session-1", "10.0.0.1", "AA-BB-CC-DD-EE-FF").is_ok());
        assert!(validate_session_action("session-1", "10.0.0.1", "").is_ok());
        assert!(validate_session_action("session-1\n", "10.0.0.1", "AABBCCDDEEFF").is_err());
        assert!(validate_session_action("session-1", "not-an-ip", "AABBCCDDEEFF").is_err());
        assert!(validate_session_action("session-1", "10.0.0.1", "not-a-mac").is_err());
    }

    #[test]
    fn extracts_only_literal_dashboard_csrf_tokens() {
        assert_eq!(
            extract_ajax_csrf_token("data: { ajaxCsrfToken : 'abc-123_X' }"),
            Some("abc-123_X".to_string())
        );
        assert_eq!(
            extract_ajax_csrf_token("var AJAXCSRFTOKEN = 'page-local-token';"),
            Some("page-local-token".to_string())
        );
        assert_eq!(
            extract_ajax_csrf_token("ajaxCsrfToken: tokenFromAnotherFunction()"),
            None
        );
        assert_eq!(extract_ajax_csrf_token("ajaxCsrfToken: [REDACTED]"), None);
    }

    #[test]
    fn parses_object_and_positional_billing_tables() {
        let usage = parse_billing_table(
            r#"{"total":1,"rows":[{"loginTime":1721091600000,"macAddress":"AABBCCDDEEFF","flow":12.5,"userRealName":"must-not-cross","userPhone":"must-not-cross"}],"summary":{"FLOW":12.5,"COU":1,"PRIVATE":"must-not-cross"}}"#,
            USAGE_RECORD_FIELDS,
            "上网记录",
        )
        .unwrap();
        assert_eq!(usage.total, 1);
        assert_eq!(usage.rows[0]["macAddress"], "AA-BB-CC-DD-EE-FF");
        assert_eq!(usage.summary["使用流量(MB)"], "12.5");
        assert_eq!(usage.summary["记录数"], "1");
        assert!(usage.rows[0]["loginTime"].contains("2024"));
        assert!(!usage.rows[0].contains_key("userRealName"));
        assert!(!usage.rows[0].contains_key("userPhone"));
        assert!(!usage.summary.contains_key("PRIVATE"));

        let access = parse_billing_table(
            r#"{"total":3,"rows":[{"macAddress":"00-00-00-00-00-00","nasIp":"0.0.0.0","nasPort":0,"flow":1},{"macAddress":"AA-BB-CC-DD-EE-FF","nasIp":"10.21.251.3","nasPort":123,"flow":2},{"macAddress":"AA-BB-CC-DD-EE-FF","nasIp":"10.21.221.97","nasPort":456,"flow":3}],"summary":{"FLOW":6,"INTERNETUPFLOW":99,"CHINANETDOWNFLOW":88}}"#,
            USAGE_RECORD_FIELDS,
            "上网记录",
        )
        .unwrap();
        assert_eq!(access.rows[0]["accessMethod"], "lgn（有线）");
        assert_eq!(access.rows[1]["accessMethod"], "bjut_wifi");
        assert_eq!(access.rows[2]["accessMethod"], "bjut-sushe");
        for row in &access.rows {
            assert!(!row.contains_key("nasIp"));
            assert!(!row.contains_key("nasPort"));
            assert!(!row.contains_key("internetUpFlow"));
            assert!(!row.contains_key("chinanetDownFlow"));
        }
        assert_eq!(access.summary.len(), 1);
        assert_eq!(access.summary["使用流量(MB)"], "6");

        let payments = parse_billing_table(
            r#"{"total":"1","rows":[[1721091600000,"扫码",20,"自助服务","成功","must-not-cross"]]}"#,
            &["paidAt", "type", "amount", "terminal", "note"],
            "充值明细",
        )
        .unwrap();
        assert_eq!(payments.rows[0]["amount"], "20");
        assert_eq!(payments.rows[0]["type"], "扫码");
        assert!(!payments.rows[0].contains_key("5"));

        assert!(parse_billing_table(
            r#"{"total":1,"rows":[{"privateOnly":"must-not-cross"}]}"#,
            USAGE_RECORD_FIELDS,
            "上网记录",
        )
        .is_err());
    }

    #[test]
    fn parses_service_options_policy_and_questions() {
        let package_html = r#"
            <a class="pick-card" data-package="6">
              <span>套餐： 测试套餐</span><span>描述： 每月测试流量</span>
            </a>
            <button data-toggle="undoPackage" data-package-name="下周期测试套餐">取消预约</button>
        "#;
        let options = parse_package_options(package_html);
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].id, "6");
        assert_eq!(options[0].name, "测试套餐");
        assert_eq!(options[0].description, "每月测试流量");
        assert!(has_tag_attribute(
            package_html,
            "data-toggle",
            "undoPackage"
        ));
        assert!(has_package_cancel_control(package_html));
        assert!(!has_package_cancel_control(
            r#"<script>$('[data-toggle="undoPackage"]')</script>"#
        ));
        assert_eq!(
            scheduled_package_name(package_html).as_deref(),
            Some("下周期测试套餐")
        );

        let policy = parse_password_policy(
            r#"<script>var passwordPolicy = JSON.parse('{"FLDMINLENGTH":10,"FLDREQUIREUPPER":1,"FLDREQUIRELOWER":1,"FLDREQUIREDIGIT":1,"FLDREQUIRESPECIAL":1}');</script>"#,
        );
        assert_eq!(policy.min_length, 10);
        assert_eq!(policy.max_length, 16);
        assert!(policy.require_uppercase);
        assert!(policy.require_special);

        let questions = parse_security_questions(
            r#"<select id="question1"><option value="0"></option><option value="1">示例问题？</option></select>"#,
        );
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].text, "示例问题？");
    }

    #[test]
    fn package_reservation_requires_a_different_next_cycle_package() {
        assert!(!package_reservation_is_distinct(
            Some("本科生10元套餐"),
            None,
        ));
        assert!(!package_reservation_is_distinct(
            Some("本科生10元套餐"),
            Some(" 本科生10元套餐 "),
        ));
        assert!(package_reservation_is_distinct(
            Some("本科生10元套餐"),
            Some("本科生20元套餐"),
        ));
    }

    #[test]
    fn parses_only_the_latest_active_package_reservation() {
        let active = parse_active_package_reservation(
            r#"{"total":1,"rows":[{"fldchangedate":1784523386000,"flddefaultname1":"本科生10元套餐","flddefaultname2":"本科生默认套餐","fldstate":"1"}]}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(active.current_package.as_deref(), Some("本科生10元套餐"));
        assert_eq!(active.scheduled_package, "本科生默认套餐");

        assert!(parse_active_package_reservation(
            r#"{"total":1,"rows":[{"flddefaultname1":"本科生10元套餐","flddefaultname2":"本科生默认套餐","fldstate":"0","fldextend":"用户手动取消"}]}"#,
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn derives_next_cycle_package_from_the_active_log_entry() {
        let dashboard = r#"
          <script>(function(user){window.user=user;})({"useFlag":1,"serviceDefault":{"id":7,"defaultName":"本科生10元套餐","extend":"当前套餐"}});</script>
        "#;
        let package_html = r#"
          <a class="pick-card" data-package="6"><span>套餐： 本科生默认套餐</span></a>
          <a class="pick-card" data-package="8"><span>套餐： 本科生20元套餐</span></a>
          <script>$('[data-toggle="undoPackage"]')</script>
        "#;
        let reservation = ActivePackageReservation {
            current_package: Some("本科生10元套餐".to_string()),
            scheduled_package: "本科生默认套餐".to_string(),
        };
        let state = parse_service_state(dashboard, "", "", package_html, "", Some(&reservation));
        assert!(state.package_scheduled);
        assert_eq!(state.scheduled_package_id.as_deref(), Some("6"));
        assert_eq!(state.scheduled_package.as_deref(), Some("本科生默认套餐"));
        assert!(state.package_options.iter().any(|option| option.id == "7"));
    }

    #[test]
    fn derives_service_state_without_treating_script_selectors_as_schedules() {
        let dashboard = r#"
          <script>(function(user){window.user=user;})({"leftMoney":3.5,"installmentFlag":999999,"useMoney":1.25,"useFlag":0,"stopReason":"余额不足","serviceDefault":{"id":7,"defaultName":"测试套餐","extend":"测试说明"}});</script>
        "#;
        let stop =
            r#"<p>下次结算日期：2026-08-01</p><script>$('[data-toggle="undoPreStop"]')</script>"#;
        let reopen =
            r#"<button id="reOpenNow">立即复通</button><button id="reOpen">预约复通</button>"#;
        let state = parse_service_state(dashboard, stop, reopen, "", "", None);
        assert_eq!(state.account_status.as_deref(), Some("停机"));
        assert!(state.can_reopen_now);
        assert!(!state.can_stop_now);
        assert_eq!(state.consume_limit.as_deref(), Some("不限制"));
        assert_eq!(state.next_settlement_date.as_deref(), Some("2026-08-01"));
        assert_eq!(state.current_package_id.as_deref(), Some("7"));
        assert_eq!(state.package_options[0].id, "7");
        assert_eq!(state.package_options[0].name, "测试套餐");
        assert!(!has_enabled_element_id(
            r#"<button id="stopNow" disabled>报停</button>"#,
            "stopNow"
        ));
    }

    #[test]
    fn validates_record_queries_without_accepting_urls_or_unbounded_dates() {
        let today = chrono::Local::now().date_naive();
        let mut query = BillingRecordQuery {
            kind: "usage".to_string(),
            page: 1,
            page_size: 10,
            start_date: Some((today - chrono::Duration::days(60)).to_string()),
            end_date: Some(today.to_string()),
            year: None,
            all: false,
        };
        let validated = validate_record_query(&query).unwrap();
        assert_eq!(validated.kind, BillingRecordKind::Usage);
        assert_eq!(validated.page_size, 10);

        query.start_date = Some((today - chrono::Duration::days(61)).to_string());
        assert!(validate_record_query(&query).is_err());
        query.kind = "https://example.com/collect".to_string();
        assert!(validate_record_query(&query).is_err());
        query.kind = "monthly".to_string();
        query.start_date = None;
        query.end_date = None;
        query.year = Some(today.year().to_string());
        assert!(validate_record_query(&query).is_ok());
    }

    #[test]
    fn parses_action_results_and_form_csrf_token() {
        assert_eq!(
            parse_action_response(r#"{"state":"success","message":"完成"}"#, "默认").unwrap(),
            "完成"
        );
        assert!(parse_action_response(r#"{"state":"fail","message":"拒绝"}"#, "默认").is_err());
        let html = r#"
          <input type="hidden" name="csrftoken" value="form-token">
          <script>$.post("doPackage", { csrftoken: 'action-token', serid: id });</script>
        "#;
        assert_eq!(form_csrf_token(html).unwrap(), "form-token");
        assert_eq!(
            action_csrf_token(html, "doPackage").unwrap(),
            "action-token"
        );
    }

    #[test]
    fn accepts_read_only_payload_wrappers_and_raw_mauth_html() {
        let records = parse_login_history(
            "\u{feff}{\"rows\":[[1721091600000,1721095200000,\"10.0.0.1\",\"\",\"000000000000\",60,1.5,3,0]]}",
        )
        .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].billing_mode, "包月");

        assert!(parse_mauth_state("<a data-toggle='oprateMauth'>开启</a>").unwrap());
        assert!(parse_billing_table("null", USAGE_RECORD_FIELDS, "上网记录")
            .unwrap()
            .rows
            .is_empty());
    }

    #[test]
    fn validates_billing_action_inputs_locally() {
        assert_eq!(validate_consume_limit(Some("12.345")).unwrap(), "12.345");
        assert!(validate_consume_limit(Some("12.3456")).is_err());
        assert_eq!(
            validate_mac_input(Some("aa-bb-cc-dd-ee-ff")).unwrap(),
            "AABBCCDDEEFF"
        );
        assert!(validate_mac_input(Some("not-a-mac")).is_err());

        let policy = BillingPasswordPolicy {
            min_length: 8,
            max_length: 16,
            require_uppercase: true,
            require_lowercase: true,
            require_digit: true,
            require_special: true,
        };
        assert!(validate_new_password(Some("NewPass1!"), "OldPass1!", &policy).is_ok());
        assert!(validate_new_password(Some("weakpass"), "OldPass1!", &policy).is_err());

        let allowed = ["1", "2", "3"]
            .into_iter()
            .map(|id| BillingSecurityQuestion {
                id: id.to_string(),
                text: format!("问题{id}"),
            })
            .collect::<Vec<_>>();
        let valid = ["1", "2", "3"]
            .into_iter()
            .map(|id| BillingQuestionAnswer {
                question_id: id.to_string(),
                answer: format!("答案{id}"),
            })
            .collect::<Vec<_>>();
        assert!(validate_question_answers(&valid, &allowed).is_ok());
        let mut duplicate = valid.clone();
        duplicate[2].question_id = "2".to_string();
        assert!(validate_question_answers(&duplicate, &allowed).is_err());
    }
}
