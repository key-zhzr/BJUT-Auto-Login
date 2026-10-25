//! 统一日志脱敏兜底。
//!
//! 在唯一的日志入口 `rust_log` 处调用一次，保证密码不会进入 `app.log`、
//! 内存日志窗口、时间线与前端。分两层：
//! 1. 已知密钥层：校园网账号密码与路由器管理密码按原文、URL 编码
//!    （大写/小写十六进制、空格转 `+`）逐一替换。
//! 2. 模式层：无论是否认识具体密钥，`user_account`/`user_password`/
//!    `token`/`session` 等凭据型 URL 参数的取值，以及 `Cookie:`、
//!    `Authorization:` 头的取值一律打码——覆盖会话 Cookie、CAS 票据、
//!    eportal 编码串等派生凭据。
//!
//! 账号用户名保持可读：用户名是排障所需信息，不属于机密；时间戳、
//! 状态码、IP 等排障信息也不受影响。

/// 原文替换的最短密钥长度。更短的密钥只由模式层兜底（凭据型参数取值、
/// 请求头取值仍会被打码），避免把时间戳、URL 等普通文本误伤。
const MIN_RAW_SECRET_LEN: usize = 4;

/// 替换标记。与 config_model 中 `[hidden]` 的思路一致，但面向日志阅读者
/// 使用中文，与日志其余文案一致。
pub(crate) const REDACTED: &str = "[已脱敏]";

/// 凭据型 URL 参数名（小写匹配）。`user_account` 在 Type1/Type2 登录 URL
/// 中承载 eportal 编码串（内含密码），必须整体打码；其余为常见凭据、
/// 会话与 CAS 票据参数。
const SENSITIVE_QUERY_PARAMS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "user_password",
    "user_account",
    "login_password",
    "j_password",
    "secret",
    "token",
    "access_token",
    "auth_token",
    "id_token",
    "session",
    "sessionid",
    "jsessionid",
    "sid",
    "ticket",
    "execution",
    "lt",
];

/// 从配置中收集需要原文打码的密钥（账号密码 + 路由器管理密码），去重、去空。
/// 会话 Cookie 不在此列：它们由模式层的请求头打码兜底，且逐条收集会拖慢热路径。
pub(crate) fn collect_secrets(config: &crate::config_model::AppConfig) -> Vec<String> {
    let mut secrets: Vec<String> = config
        .accounts
        .iter()
        .map(|account| account.pass.clone())
        .collect();
    if let Some(router) = &config.saved_router {
        secrets.push(router.password.clone());
    }
    secrets.retain(|secret| !secret.is_empty());
    secrets.sort();
    secrets.dedup();
    secrets
}

/// 对一条日志消息做完整脱敏：先打码凭据型参数与请求头（保留参数名便于
/// 排障），再对已知密钥做原文与 URL 编码形式的替换。
pub(crate) fn redact_message(message: &str, secrets: &[String]) -> String {
    let redacted = redact_sensitive_headers(&redact_sensitive_query_params(message));
    redact_known_secrets(&redacted, secrets)
}

/// 打码 `参数名=取值` 中取值属于凭据型参数的部分；参数名保持可见。
fn redact_sensitive_query_params(message: &str) -> String {
    let bytes = message.as_bytes();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if !is_name_char(bytes[index]) || (index > 0 && is_name_char(bytes[index - 1])) {
            index += 1;
            continue;
        }
        let name_start = index;
        while index < bytes.len() && is_name_char(bytes[index]) {
            index += 1;
        }
        let name = &message[name_start..index];
        if index >= bytes.len() || bytes[index] != b'=' {
            continue;
        }
        if !SENSITIVE_QUERY_PARAMS.contains(&name.to_ascii_lowercase().as_str()) {
            continue;
        }
        index += 1;
        let value_start = index;
        while index < bytes.len() && !is_value_delimiter(bytes[index]) {
            index += 1;
        }
        if index > value_start {
            spans.push((value_start, index));
        }
    }
    assemble(message, &spans)
}

/// 打码 `Cookie:`/`Authorization:` 头的取值（含 `Set-Cookie:`）。只在取值
/// 具备凭据形态（长 token 或含 `=`，或声明了 Bearer/Basic）时整体打码，
/// 避免误伤「读取 Cookie: 5 条」这类说明文字。
fn redact_sensitive_headers(message: &str) -> String {
    const HEADER_NAMES: [&str; 2] = ["cookie:", "authorization:"];
    let lowered: Vec<u8> = message
        .bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .collect();
    let bytes = message.as_bytes();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for header in HEADER_NAMES {
        let mut search_from = 0;
        while let Some(relative) = find_subslice(&lowered[search_from..], header.as_bytes()) {
            let start = search_from + relative;
            search_from = start + header.len();
            let boundary_ok = start == 0 || !(bytes[start - 1].is_ascii_alphanumeric());
            if !boundary_ok {
                continue;
            }
            let mut value_start = start + header.len();
            while value_start < bytes.len() && bytes[value_start] == b' ' {
                value_start += 1;
            }
            let mut value_end = value_start;
            while value_end < bytes.len() && bytes[value_end] != b'\n' && bytes[value_end] != b'\r'
            {
                value_end += 1;
            }
            let value = &message[value_start..value_end];
            if header_value_looks_credential(value) {
                if value_end > value_start {
                    spans.push((value_start, value_end));
                }
                search_from = value_end.max(search_from);
            }
        }
    }
    spans.sort_unstable();
    assemble(message, &spans)
}

/// 请求头取值是否具备凭据形态。
fn header_value_looks_credential(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let lowered = value.to_ascii_lowercase();
    if lowered.contains("bearer ") || lowered.contains("basic ") {
        return true;
    }
    if lowered.contains("sessionid") || lowered.contains("token") {
        return true;
    }
    value.len() >= 16
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'=' | b'/' | b'+' | b'.' | b'_' | b'-' | b';' | b':' | b'|' | b'%' | b' '
                )
        })
}

/// 已知密钥层：原文替换 + URL 编码变体替换。
fn redact_known_secrets(message: &str, secrets: &[String]) -> String {
    let mut redacted = message.to_string();
    for secret in secrets {
        if secret.chars().count() < MIN_RAW_SECRET_LEN {
            continue;
        }
        redacted = redacted.replace(secret.as_str(), REDACTED);
        for encoded in encoded_secret_forms(secret) {
            redacted = redacted.replace(encoded.as_str(), REDACTED);
        }
    }
    redacted
}

/// 生成一个密钥在 URL 中可能出现的编码形态（去重、不与原文重复）。
fn encoded_secret_forms(secret: &str) -> Vec<String> {
    let bytes = secret.as_bytes();
    let mut forms = Vec::new();
    for uppercase in [true, false] {
        for plus_space in [false, true] {
            let form = percent_encode(bytes, uppercase, plus_space, false);
            if form != secret && !forms.contains(&form) {
                forms.push(form);
            }
        }
    }
    let javascript_style = percent_encode(bytes, true, false, true);
    if javascript_style != secret && !forms.contains(&javascript_style) {
        forms.push(javascript_style);
    }
    forms
}

/// 百分号编码：默认保留 `A-Za-z0-9-_.~`，`keep_javascript_extras` 额外保留
/// `!'()*`（对齐 encodeURIComponent）；`plus_space` 将空格编码为 `+`。
fn percent_encode(
    bytes: &[u8],
    uppercase: bool,
    plus_space: bool,
    keep_javascript_extras: bool,
) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &byte in bytes {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~')
            || (keep_javascript_extras && matches!(byte, b'!' | b'\'' | b'(' | b')' | b'*'));
        if keep {
            out.push(byte as char);
        } else if plus_space && byte == b' ' {
            out.push('+');
        } else if uppercase {
            out.push_str(&format!("%{byte:02X}"));
        } else {
            out.push_str(&format!("%{byte:02x}"));
        }
    }
    out
}

fn is_name_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn is_value_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'&' | b' ' | b'"' | b'\'' | b'<' | b'>' | b';' | b'\n' | b'\r' | b'\t'
    )
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 把命中的取值区间替换为标记后重组消息。区间边界均落在 ASCII 字符上，
/// 切片不会破坏多字节字符。
fn assemble(message: &str, spans: &[(usize, usize)]) -> String {
    if spans.is_empty() {
        return message.to_string();
    }
    let mut out = String::with_capacity(message.len());
    let mut copied_until = 0;
    for &(start, end) in spans {
        if start < copied_until {
            continue;
        }
        out.push_str(&message[copied_until..start]);
        out.push_str(REDACTED);
        copied_until = end;
    }
    out.push_str(&message[copied_until..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_model::{Account, AppConfig};

    fn secrets(list: &[&str]) -> Vec<String> {
        list.iter().map(|secret| secret.to_string()).collect()
    }

    #[test]
    fn known_password_is_redacted_while_account_stays_readable() {
        let redacted = redact_message(
            "账号 2023021234 登录失败，使用密码 S3cret!pw 进行重试",
            &secrets(&["S3cret!pw"]),
        );
        assert!(!redacted.contains("S3cret!pw"));
        assert!(redacted.contains(REDACTED));
        assert!(
            redacted.contains("账号 2023021234"),
            "用户名必须保持可读：{redacted}"
        );
    }

    #[test]
    fn percent_encoded_password_is_redacted() {
        let redacted = redact_message(
            "hint=p%40ss%20word&ok=1 以及 user_password=p%40ss%20word",
            &secrets(&["p@ss word"]),
        );
        assert!(!redacted.contains("p%40ss%20word"));
        assert!(!redacted.contains("p@ss word"));
        assert!(
            redacted.contains("hint="),
            "非凭据参数名必须保留：{redacted}"
        );
        assert!(
            redacted.contains("&ok=1"),
            "非凭据参数不能被误伤：{redacted}"
        );
        assert!(redacted.contains("user_password="));
    }

    #[test]
    fn space_and_plus_encoded_forms_are_redacted() {
        let redacted = redact_message(
            "token=Ab%20c and token=Ab+c and raw Ab c",
            &secrets(&["Ab c"]),
        );
        assert!(!redacted.contains("Ab%20c"));
        assert!(!redacted.contains("Ab+c"));
        assert!(!redacted.contains("raw Ab c"));
    }

    #[test]
    fn credential_query_values_are_redacted_even_without_known_secrets() {
        let redacted = redact_message(
            "GET https://10.21.221.98:802/eportal/portal/login?user_account=5B6E7C8F9A&user_password=9f86d081882c&login_method=1",
            &[],
        );
        assert!(redacted.contains("user_account="));
        assert!(redacted.contains("user_password="));
        assert!(!redacted.contains("5B6E7C8F9A"));
        assert!(!redacted.contains("9f86d081882c"));
        assert!(
            redacted.contains("login_method=1"),
            "排障参数必须保留：{redacted}"
        );
    }

    #[test]
    fn cookie_and_authorization_values_are_redacted() {
        let redacted = redact_message(
            "POST /auth\nCookie: JSESSIONID=abc123def456789; route=1\nAuthorization: Bearer eyJhbGciOiJIUzI1NiJ9\n",
            &[],
        );
        assert!(!redacted.contains("abc123def456789"));
        assert!(!redacted.contains("eyJhbGciOiJIUzI1NiJ9"));
        assert!(redacted.contains("Cookie:"));
        assert!(redacted.contains("Authorization:"));
    }

    #[test]
    fn prose_mentions_of_cookies_survive() {
        let message = "读取 Cookie: 5 条，解析 Authorization 完成";
        assert_eq!(redact_message(message, &[]), message);
    }

    #[test]
    fn short_secrets_are_only_redacted_in_credential_contexts() {
        let redacted = redact_message("余额 ab 元，pwd=ab", &secrets(&["ab"]));
        assert!(
            redacted.contains("余额 ab 元"),
            "过短密钥不应伤及普通文本：{redacted}"
        );
        assert!(redacted.contains("pwd="));
        assert!(!redacted.contains("pwd=ab"));
    }

    #[test]
    fn non_ascii_password_encoded_forms_are_redacted() {
        let secret = "密码＃12";
        let redacted = redact_message("token=%E5%AF%86%E7%A0%81%EF%BC%8312", &secrets(&[secret]));
        assert!(!redacted.contains("%E5%AF%86%E7%A0%81%EF%BC%8312"));
    }

    #[test]
    fn messages_without_hits_are_untouched() {
        let message = "探测完成：10.21.221.98 状态 ready，耗时 38 ms";
        assert_eq!(redact_message(message, &secrets(&["S3cret!pw"])), message);
    }

    #[test]
    fn collect_secrets_gathers_accounts_and_router_and_dedupes() {
        // AppConfig 没有 Default derive，用空 JSON 反序列化得到缺省配置，
        // 对后续新增的带默认值字段保持稳健。
        let mut config: AppConfig =
            serde_json::from_str("{}").expect("空配置对象应能反序列化为 AppConfig");
        config.accounts.push(Account {
            user: "u1".into(),
            pass: "alpha-pass".into(),
            is_default: true,
            is_disabled: None,
        });
        config.accounts.push(Account {
            user: "u2".into(),
            pass: "alpha-pass".into(),
            is_default: false,
            is_disabled: None,
        });
        let mut router = crate::config_model::SavedRouter {
            address: "10.0.0.1".into(),
            username: "root".into(),
            password: "beta-pass".into(),
            certificate: None,
            allow_http: false,
            binding: None,
        };
        config.saved_router = Some(router.clone());
        let collected = collect_secrets(&config);
        assert_eq!(
            collected,
            vec!["alpha-pass".to_string(), "beta-pass".to_string()]
        );
        router.password.clear();
        config.saved_router = Some(router);
        let collected = collect_secrets(&config);
        assert_eq!(collected, vec!["alpha-pass".to_string()]);
    }

    #[test]
    fn router_password_is_redacted() {
        let redacted = redact_message(
            "连接路由器 192.168.1.1 失败：Router-Pw2026 不正确",
            &secrets(&["Router-Pw2026"]),
        );
        assert!(!redacted.contains("Router-Pw2026"));
        assert!(
            redacted.contains("192.168.1.1"),
            "排障 IP 必须保留：{redacted}"
        );
    }
}
