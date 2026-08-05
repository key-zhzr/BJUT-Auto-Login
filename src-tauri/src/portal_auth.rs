use super::{
    check_internet_from_source, query_campus_dns_ipv4, redact_request_error, route_source_ipv4,
    VpnCompatibility, LGN6_HOST, LGN_HOST, WLGN_HOST,
};
use crate::network_trust::{campus_wifi_kind, CampusWifiKind};
use reqwest::{Client, Url};
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) const AMBIGUOUS_LOGIN_RESULT: &str = "认证结果暂无法确认";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoginType {
    Type1,
    Type2,
    Type3,
    Unknown,
}

impl LoginType {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Type1 => "Type1_221_98",
            Self::Type2 => "Type2_251_3",
            Self::Type3 => "Type3_172_30",
            Self::Unknown => "Unknown",
        }
    }
}

fn random_request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:04}", nanos % 10_000)
}

fn login_type_hint(ssid: &str) -> Option<LoginType> {
    match campus_wifi_kind(ssid) {
        Some(CampusWifiKind::Dormitory) => Some(LoginType::Type1),
        Some(CampusWifiKind::Public) => Some(LoginType::Type2),
        None => None,
    }
}

pub(crate) async fn portal_client(
    compatibility: VpnCompatibility,
    login_type: &LoginType,
    timeout: Duration,
) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .use_rustls_tls();
    let hosts: Vec<(&str, Vec<Ipv4Addr>)> = match login_type {
        LoginType::Type2 => vec![(WLGN_HOST, vec![Ipv4Addr::new(10, 21, 251, 3)])],
        LoginType::Type3 => vec![
            (
                LGN_HOST,
                vec![
                    Ipv4Addr::new(172, 30, 201, 2),
                    Ipv4Addr::new(172, 30, 201, 10),
                ],
            ),
            (
                LGN6_HOST,
                vec![
                    Ipv4Addr::new(172, 30, 201, 2),
                    Ipv4Addr::new(172, 30, 201, 10),
                ],
            ),
        ],
        _ => Vec::new(),
    };
    if matches!(
        compatibility,
        VpnCompatibility::Low | VpnCompatibility::High
    ) {
        for (host, fixed_addresses) in hosts {
            let addresses = if compatibility == VpnCompatibility::Low {
                let host_owned = host.to_string();
                tokio::task::spawn_blocking(move || query_campus_dns_ipv4(&host_owned))
                    .await
                    .map_err(|error| format!("校园网 DNS 任务失败：{error}"))??
            } else {
                fixed_addresses
            };
            let socket_addresses: Vec<SocketAddr> = addresses
                .into_iter()
                .map(|address| SocketAddr::new(IpAddr::V4(address), 443))
                .collect();
            builder = builder.resolve_to_addrs(host, &socket_addresses);
        }
    }
    builder.build().map_err(redact_request_error)
}

pub(crate) fn portal_probe_urls(
    compatibility: VpnCompatibility,
    login_type: &LoginType,
) -> Vec<String> {
    match login_type {
        LoginType::Type1 if compatibility == VpnCompatibility::Maximum => {
            vec!["http://10.21.221.98:801/eportal/portal/login".to_string()]
        }
        LoginType::Type1 => vec!["https://10.21.221.98:802/eportal/portal/login".to_string()],
        LoginType::Type2 if compatibility == VpnCompatibility::Maximum => {
            vec!["http://10.21.251.3/drcom/login".to_string()]
        }
        LoginType::Type2 => vec!["https://wlgn.bjut.edu.cn/drcom/login".to_string()],
        LoginType::Type3 if compatibility == VpnCompatibility::Maximum => {
            let primary = lgn_user_info_url(compatibility);
            let secondary = primary.replacen("172.30.201.2", "172.30.201.10", 1);
            vec![primary, secondary]
        }
        LoginType::Type3 => vec![
            lgn_user_info_url(compatibility),
            "https://lgn6.bjut.edu.cn/V6?https://lgn.bjut.edu.cn".to_string(),
        ],
        LoginType::Unknown => Vec::new(),
    }
}

async fn probe_login_type(
    compatibility: VpnCompatibility,
    login_type: LoginType,
) -> Option<LoginType> {
    let client = portal_client(compatibility, &login_type, Duration::from_millis(1800))
        .await
        .ok()?;
    for url in portal_probe_urls(compatibility, &login_type) {
        let Ok(response) = client
            .get(&url)
            .header("Cache-Control", "no-cache, no-store")
            .send()
            .await
        else {
            continue;
        };
        // Reachability of the fixed campus endpoint is the only response
        // characteristic currently documented for probes. Reject redirects,
        // server errors and empty responses rather than treating any completed
        // TCP/TLS request as a confirmed protocol.
        let status_ok = response.status().is_success();
        let body = response.text().await.unwrap_or_default();
        if status_ok && probe_body_matches(&login_type, &url, &body) {
            return Some(login_type);
        }
    }
    None
}

fn jsonp_object(text: &str) -> Option<Value> {
    let start = text.find('(')?;
    let end = text.rfind(')').filter(|end| *end > start)?;
    serde_json::from_str(&text[start + 1..end]).ok()
}

fn probe_body_matches(login_type: &LoginType, url: &str, body: &str) -> bool {
    if body.trim().is_empty() {
        return false;
    }
    let normalized = body.to_ascii_lowercase();
    match login_type {
        LoginType::Type1 => {
            normalized.contains("eportal")
                || normalized.contains("user_account")
                || jsonp_object(body).is_some_and(|value| value.get("result").is_some())
        }
        LoginType::Type2 => {
            normalized.contains("drcom")
                || normalized.contains("ddddd")
                || normalized.contains("0mkkey")
                || jsonp_object(body).is_some_and(|value| value.get("result").is_some())
        }
        LoginType::Type3 if url.contains("loadUserInfo") => {
            jsonp_object(body).is_some_and(|value| {
                value.get("code").is_some() && value.get("user_info").is_some_and(Value::is_object)
            })
        }
        LoginType::Type3 => {
            normalized.contains("name=\"v6ip\"")
                || normalized.contains("name='v6ip'")
                || normalized.contains("lgn.bjut.edu.cn")
        }
        LoginType::Unknown => false,
    }
}

pub(crate) async fn detect_login_type_rust(
    compatibility: VpnCompatibility,
    ssid: &str,
    transport: &str,
) -> LoginType {
    if let Some(hint) = login_type_hint(ssid) {
        return hint;
    }
    let candidates: Vec<LoginType> = if transport.eq_ignore_ascii_case("wifi") {
        vec![LoginType::Type1, LoginType::Type2]
    } else if transport.eq_ignore_ascii_case("ethernet") {
        vec![LoginType::Type3]
    } else {
        vec![LoginType::Type1, LoginType::Type2, LoginType::Type3]
    };
    let probes = candidates
        .into_iter()
        .map(|login_type| probe_login_type(compatibility, login_type));
    let confirmed: Vec<LoginType> = futures_util::future::join_all(probes)
        .await
        .into_iter()
        .flatten()
        .collect();
    if confirmed.len() == 1 {
        confirmed.into_iter().next().unwrap_or(LoginType::Unknown)
    } else {
        LoginType::Unknown
    }
}

fn parse_dr_response(text: &str) -> Result<(bool, String), String> {
    let start = text
        .find('(')
        .ok_or_else(|| format!("{AMBIGUOUS_LOGIN_RESULT}：登录网关未返回预期 JSONP 响应"))?;
    let end = text
        .rfind(')')
        .filter(|end| *end > start)
        .ok_or_else(|| format!("{AMBIGUOUS_LOGIN_RESULT}：登录网关返回的 JSONP 不完整"))?;
    let data: Value = serde_json::from_str(&text[start + 1..end])
        .map_err(|_| format!("{AMBIGUOUS_LOGIN_RESULT}：登录网关响应无法解析"))?;
    let result = data
        .get("result")
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str()?.parse::<i64>().ok())
        })
        .ok_or_else(|| format!("{AMBIGUOUS_LOGIN_RESULT}：登录网关未返回 result 字段"))?;
    if !matches!(result, 0 | 1) {
        return Err(format!(
            "{AMBIGUOUS_LOGIN_RESULT}：登录网关返回了未知 result 值 {result}"
        ));
    }
    let message = data
        .get("msga")
        .or_else(|| data.get("msg"))
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string())
        })
        .unwrap_or_else(|| {
            if result == 1 {
                "Portal协议认证成功！".to_string()
            } else {
                "登录被认证网关拒绝".to_string()
            }
        });
    Ok((result == 1, message))
}

fn find_v6ip(html: &str) -> String {
    for (name, value) in [("name=\"v6ip\"", "value=\""), ("name='v6ip'", "value='")] {
        if let Some(name_pos) = html.find(name) {
            let source = &html[name_pos..];
            if let Some(value_pos) = source.find(value) {
                let start = value_pos + value.len();
                let quote = value.chars().last().unwrap_or('"');
                if let Some(end) = source[start..].find(quote) {
                    return source[start..start + end].to_string();
                }
            }
        }
    }
    String::new()
}

pub(crate) fn lgn_user_info_url(compatibility: VpnCompatibility) -> String {
    let query = format!(
        "callback=726427262624&lang=6c7e3b7578&program_index=79225954737327212323222f212e2723&page_index=755e577b7c4e27212323222f212e2320&user_account=&wlan_user_ip=&wlan_user_ipv6=&wlan_user_mac=262626262626262626262626&jsVersion=22384e&encrypt=1&v={}&lang=zh",
        random_request_id()
    );
    if compatibility == VpnCompatibility::Maximum {
        format!("http://172.30.201.2:801/eportal/portal/page/loadUserInfo?{query}")
    } else {
        format!("https://lgn.bjut.edu.cn:802/eportal/portal/page/loadUserInfo?{query}")
    }
}

async fn confirm_lgn_account(
    client: &Client,
    compatibility: VpnCompatibility,
    expected_account: &str,
) -> bool {
    let Ok(response) = client.get(lgn_user_info_url(compatibility)).send().await else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    let Ok(text) = response.text().await else {
        return false;
    };
    let Some(start) = text.find('(') else {
        return false;
    };
    let Some(end) = text.rfind(')').filter(|end| *end > start) else {
        return false;
    };
    let Ok(data) = serde_json::from_str::<Value>(&text[start + 1..end]) else {
        return false;
    };
    data.get("code").and_then(Value::as_i64) == Some(1)
        && data
            .get("user_info")
            .and_then(|info| info.get("account"))
            .and_then(Value::as_str)
            .is_some_and(|account| account == expected_account)
}

async fn login_lgn_once(
    client: &Client,
    first_url: &str,
    second_url: &str,
    user: &str,
    pass: &str,
    compatibility: VpnCompatibility,
) -> Result<(bool, String), String> {
    let first_response = client
        .post(first_url)
        .form(&[
            ("DDDDD", user),
            ("upass", pass),
            ("v46s", "0"),
            ("0MKKey", ""),
        ])
        .send()
        .await
        .map_err(redact_request_error)?;
    if !first_response.status().is_success() {
        return Err(format!(
            "有线登录第一步返回 HTTP {}",
            first_response.status()
        ));
    }
    let html = first_response.text().await.map_err(redact_request_error)?;
    let v6ip = find_v6ip(&html);
    if v6ip.is_empty() {
        return Err("有线登录页未返回动态 IPv6 地址".to_string());
    }

    let final_response = client
        .post(second_url)
        .form(&[
            ("DDDDD", user),
            ("upass", pass),
            ("0MKKey", "Login"),
            ("v6ip", v6ip.as_str()),
        ])
        .send()
        .await
        .map_err(redact_request_error)?;
    if !final_response.status().is_success() {
        return Err(format!(
            "有线登录第二步返回 HTTP {}",
            final_response.status()
        ));
    }
    let final_html = final_response.text().await.map_err(redact_request_error)?;
    let normalized = final_html.to_ascii_lowercase();
    if normalized.contains("dispqianfei")
        || normalized.contains("ldap auth error")
        || normalized.contains("userid error")
        || final_html.contains("账号不存在")
        || final_html.contains("密码错误")
        || final_html.contains("余额不足")
    {
        return Ok((false, "登录失败，请检查账号密码或余额".to_string()));
    }
    if confirm_lgn_account(client, compatibility, user).await {
        return Ok((true, "Portal协议认证成功！".to_string()));
    }
    let route_source = route_source_ipv4("172.30.201.2:443");
    if check_internet_from_source((!route_source.is_empty()).then_some(route_source.as_str())).await
    {
        return Ok((true, "Portal协议认证成功！".to_string()));
    }
    Err(format!(
        "{AMBIGUOUS_LOGIN_RESULT}：请求已提交，但账号信息和互联网连通性均未能确认成功；已停止继续尝试其他账号"
    ))
}

fn type1_login_url(compatibility: VpnCompatibility, user: &str, pass: &str) -> Result<Url, String> {
    let base = if compatibility == VpnCompatibility::Maximum {
        "http://10.21.221.98:801/eportal/portal/login"
    } else {
        "https://10.21.221.98:802/eportal/portal/login"
    };
    let mut url = Url::parse(base).map_err(|error| error.to_string())?;
    let account = format!("{user}@campus");
    url.query_pairs_mut()
        .append_pair("callback", "dr1003")
        .append_pair("login_method", "1")
        .append_pair("user_account", &account)
        .append_pair("user_password", pass)
        .append_pair("wlan_user_ip", "")
        .append_pair("wlan_user_ipv6", "")
        .append_pair("wlan_user_mac", "000000000000")
        .append_pair("wlan_ac_ip", "")
        .append_pair("wlan_ac_name", "")
        .append_pair("jsVersion", "4.2.1")
        .append_pair("terminal_type", "1")
        .append_pair("lang", "zh-cn")
        .append_pair("v", &random_request_id());
    Ok(url)
}

fn type2_login_url(compatibility: VpnCompatibility, user: &str, pass: &str) -> Result<Url, String> {
    let base = if compatibility == VpnCompatibility::Maximum {
        "http://10.21.251.3/drcom/login"
    } else {
        "https://wlgn.bjut.edu.cn/drcom/login"
    };
    let mut url = Url::parse(base).map_err(|error| error.to_string())?;
    url.query_pairs_mut()
        .append_pair("callback", "dr1002")
        .append_pair("DDDDD", user)
        .append_pair("upass", pass)
        .append_pair("0MKKey", "123456")
        .append_pair("R1", "0")
        .append_pair("R2", "")
        .append_pair("R3", "0")
        .append_pair("R6", "0")
        .append_pair("para", "00")
        .append_pair("v6ip", "")
        .append_pair("terminal_type", "1")
        .append_pair("lang", "zh-cn")
        .append_pair("jsVersion", "4.1")
        .append_pair("v", &random_request_id());
    Ok(url)
}

async fn get_jsonp_login(client: &Client, url: Url) -> Result<(bool, String), String> {
    let response = client.get(url).send().await.map_err(redact_request_error)?;
    if !response.status().is_success() {
        return Err(format!("登录网关返回 HTTP {}", response.status()));
    }
    let text = response.text().await.map_err(redact_request_error)?;
    parse_dr_response(&text)
}

pub(crate) async fn login_to_campus_network_rust(
    login_type: LoginType,
    user: &str,
    pass: &str,
    compatibility: VpnCompatibility,
) -> Result<(bool, String), String> {
    // The documented lgn flow requires the HTTPS hostnames. For the maximum
    // compatibility setting, keep TLS/SNI and pin the known addresses instead
    // of constructing the invalid http://IP/V6?http://IP flow.
    let client_compatibility =
        if login_type == LoginType::Type3 && compatibility == VpnCompatibility::Maximum {
            VpnCompatibility::High
        } else {
            compatibility
        };
    let client = portal_client(client_compatibility, &login_type, Duration::from_secs(5)).await?;
    match login_type {
        LoginType::Type1 => {
            get_jsonp_login(&client, type1_login_url(compatibility, user, pass)?).await
        }
        LoginType::Type2 => {
            get_jsonp_login(&client, type2_login_url(compatibility, user, pass)?).await
        }
        LoginType::Type3 => {
            login_lgn_once(
                &client,
                "https://lgn6.bjut.edu.cn/V6?https://lgn.bjut.edu.cn",
                "https://lgn.bjut.edu.cn",
                user,
                pass,
                compatibility,
            )
            .await
        }
        LoginType::Unknown => Err("未设定的登录类型".to_string()),
    }
}

pub(crate) fn login_result_is_ambiguous(error: &str) -> bool {
    error.starts_with(AMBIGUOUS_LOGIN_RESULT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantically_valid_jsonp_results_are_required() {
        assert_eq!(
            parse_dr_response(r#"dr1003({"result":1,"msg":"ok"});"#).unwrap(),
            (true, "ok".to_string())
        );
        assert_eq!(
            parse_dr_response(r#"dr1002({"result":0,"msga":"密码错误"});"#).unwrap(),
            (false, "密码错误".to_string())
        );
        assert!(parse_dr_response("not json").is_err());
        assert!(parse_dr_response(r#"dr1003({"result":2,"msg":"unknown"});"#).is_err());
    }

    #[test]
    fn login_urls_use_the_documented_ports_and_fields() {
        let dorm = type1_login_url(VpnCompatibility::Maximum, "25000000", "p!").unwrap();
        assert_eq!(dorm.port(), Some(801));
        assert_eq!(dorm.scheme(), "http");
        assert!(dorm.as_str().contains("terminal_type=1"));
        assert!(dorm.as_str().contains("user_account=25000000%40campus"));

        let wifi = type2_login_url(VpnCompatibility::High, "25000000", "p!").unwrap();
        assert_eq!(wifi.scheme(), "https");
        assert_eq!(wifi.host_str(), Some(WLGN_HOST));
        assert!(wifi.as_str().contains("callback=dr1002"));
    }

    #[test]
    fn ssid_and_transport_hints_are_specific() {
        assert_eq!(
            login_type_hint("bjut-sushe--5G-bEY5"),
            Some(LoginType::Type1)
        );
        assert_eq!(
            login_type_hint("bjut-suahe-5G-6Y6m"),
            Some(LoginType::Type1)
        );
        assert_eq!(login_type_hint("bjut_wifi"), Some(LoginType::Type2));
        assert_eq!(login_type_hint("CU_bjut-sushe-28Au"), None);
        assert_eq!(login_type_hint("room-bjut-sushe-5g"), None);
        assert_eq!(login_type_hint("bjut_wifi_guest"), None);
        assert_eq!(login_type_hint(""), None);
    }

    #[test]
    fn probe_endpoints_keep_http_and_https_ports_distinct() {
        assert_eq!(
            portal_probe_urls(VpnCompatibility::Maximum, &LoginType::Type1),
            vec!["http://10.21.221.98:801/eportal/portal/login"]
        );
        assert_eq!(
            portal_probe_urls(VpnCompatibility::Minimum, &LoginType::Type1),
            vec!["https://10.21.221.98:802/eportal/portal/login"]
        );
        assert!(
            portal_probe_urls(VpnCompatibility::Maximum, &LoginType::Type3)
                .iter()
                .all(|url| url.contains(":801/"))
        );
    }

    #[test]
    fn probes_require_protocol_specific_response_fingerprints() {
        assert!(probe_body_matches(
            &LoginType::Type1,
            "http://10.21.221.98:801/eportal/portal/login",
            r#"dr1003({"result":0,"msg":"missing parameters"});"#,
        ));
        assert!(probe_body_matches(
            &LoginType::Type2,
            "https://wlgn.bjut.edu.cn/drcom/login",
            "<form><input name=\"DDDDD\"><input name=\"0MKKey\"></form>",
        ));
        assert!(probe_body_matches(
            &LoginType::Type3,
            "http://172.30.201.2:801/eportal/portal/page/loadUserInfo",
            r#"dr1002({"code":1,"user_info":{"account":"25000000"},"user_info_lang":{"account":"账号"}});"#,
        ));
        assert!(!probe_body_matches(
            &LoginType::Type3,
            "http://172.30.201.2:801/eportal/portal/page/loadUserInfo",
            r#"dr1002({"code":1,"user_info_lang":{"account":"账号"}});"#,
        ));
        assert!(!probe_body_matches(
            &LoginType::Type1,
            "http://10.21.221.98:801/eportal/portal/login",
            "generic reverse proxy error page",
        ));
    }
}
