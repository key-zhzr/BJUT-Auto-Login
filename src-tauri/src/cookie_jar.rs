use reqwest::header::{HeaderMap, SET_COOKIE};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct StoredCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub host_only: bool,
    pub path: String,
    pub expires_at: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CookieJar {
    values: BTreeMap<(String, String, String), StoredCookie>,
}

impl CookieJar {
    pub(crate) fn from_records(records: impl IntoIterator<Item = StoredCookie>) -> Self {
        let now = unix_timestamp();
        let mut jar = Self::default();
        for cookie in records {
            if cookie.name.is_empty()
                || cookie.domain.is_empty()
                || !cookie.path.starts_with('/')
                || cookie.expires_at.is_some_and(|expires| expires <= now)
            {
                continue;
            }
            jar.values.insert(
                (
                    cookie.domain.clone(),
                    cookie.path.clone(),
                    cookie.name.clone(),
                ),
                cookie,
            );
        }
        jar
    }

    pub(crate) fn records(&self) -> impl Iterator<Item = &StoredCookie> {
        self.values.values()
    }

    pub(crate) fn merge(&mut self, other: Self) {
        self.values.extend(other.values);
    }

    pub(crate) fn absorb(&mut self, url: &Url, headers: &HeaderMap) {
        let Some(response_host) = url.host_str().map(str::to_ascii_lowercase) else {
            return;
        };
        for raw in headers.get_all(SET_COOKIE).iter() {
            let Ok(raw) = raw.to_str() else { continue };
            let mut parts = raw.split(';');
            let Some(pair) = parts.next() else { continue };
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
            let mut domain = response_host.clone();
            let mut host_only = true;
            let mut path = default_cookie_path(url.path());
            let mut expired = value.trim().is_empty();
            let mut expires_at = None;
            let mut secure = false;
            let mut http_only = false;
            let mut max_age_seen = false;
            for attribute in parts {
                let (key, attribute_value) = attribute
                    .trim()
                    .split_once('=')
                    .map(|(key, value)| (key.trim(), value.trim()))
                    .unwrap_or((attribute.trim(), ""));
                match key.to_ascii_lowercase().as_str() {
                    "domain" => {
                        let candidate =
                            attribute_value.trim_start_matches('.').to_ascii_lowercase();
                        if response_host == candidate
                            || response_host.ends_with(&format!(".{candidate}"))
                        {
                            domain = candidate;
                            host_only = false;
                        } else {
                            domain.clear();
                        }
                    }
                    "path" if attribute_value.starts_with('/') => {
                        path = attribute_value.to_string()
                    }
                    "max-age" => {
                        max_age_seen = true;
                        match attribute_value.parse::<i64>() {
                            Ok(age) if age <= 0 => expired = true,
                            Ok(age) => expires_at = unix_timestamp().checked_add(age),
                            Err(_) => {}
                        }
                    }
                    "expires" if !max_age_seen => {
                        if let Ok(date) = chrono::DateTime::parse_from_rfc2822(attribute_value) {
                            let timestamp = date.timestamp();
                            expired = timestamp <= unix_timestamp();
                            expires_at = Some(timestamp);
                        }
                    }
                    "secure" => secure = true,
                    "httponly" => http_only = true,
                    _ => {}
                }
            }
            if domain.is_empty() {
                continue;
            }
            let key = (domain.clone(), path.clone(), name.to_string());
            if expired {
                self.values.remove(&key);
            } else {
                self.values.insert(
                    key,
                    StoredCookie {
                        name: name.to_string(),
                        value: value.trim().to_string(),
                        domain,
                        host_only,
                        path,
                        expires_at,
                        secure,
                        http_only,
                    },
                );
            }
        }
    }

    pub(crate) fn header(&self, url: &Url) -> Option<String> {
        let host = url.host_str()?.to_ascii_lowercase();
        let path = url.path();
        let secure_request = url.scheme() == "https";
        let mut cookies = self
            .values
            .values()
            .filter(|cookie| {
                let domain_matches = if cookie.host_only {
                    host == cookie.domain
                } else {
                    host == cookie.domain || host.ends_with(&format!(".{}", cookie.domain))
                };
                domain_matches
                    && cookie_path_matches(path, &cookie.path)
                    && (!cookie.secure || secure_request)
                    && cookie
                        .expires_at
                        .is_none_or(|expires| expires > unix_timestamp())
            })
            .collect::<Vec<_>>();
        cookies.sort_by_key(|cookie| std::cmp::Reverse(cookie.path.len()));
        (!cookies.is_empty()).then(|| {
            cookies
                .into_iter()
                .map(|cookie| format!("{}={}", cookie.name, cookie.value))
                .collect::<Vec<_>>()
                .join("; ")
        })
    }
}

fn unix_timestamp() -> i64 {
    chrono::Utc::now().timestamp()
}

fn default_cookie_path(path: &str) -> String {
    if !path.starts_with('/') || path == "/" {
        return "/".to_string();
    }
    path.rsplit_once('/')
        .map(|(directory, _)| if directory.is_empty() { "/" } else { directory })
        .unwrap_or("/")
        .to_string()
}

fn cookie_path_matches(request: &str, cookie: &str) -> bool {
    request == cookie
        || request
            .strip_prefix(cookie)
            .is_some_and(|suffix| cookie.ends_with('/') || suffix.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obeys_domain_path_secure_and_deletion_rules() {
        let mut jar = CookieJar::default();
        let url = Url::parse("https://cas.bjut.edu.cn/login").unwrap();
        let mut headers = HeaderMap::new();
        headers.append(SET_COOKIE, "host=value; Path=/; Secure".parse().unwrap());
        headers.append(
            SET_COOKIE,
            "shared=value; Domain=.bjut.edu.cn; Path=/api; Secure"
                .parse()
                .unwrap(),
        );
        jar.absorb(&url, &headers);
        assert_eq!(
            jar.header(&Url::parse("https://cas.bjut.edu.cn/login").unwrap())
                .as_deref(),
            Some("host=value")
        );
        assert_eq!(
            jar.header(&Url::parse("https://uc.bjut.edu.cn/api/status").unwrap())
                .as_deref(),
            Some("shared=value")
        );
        assert!(jar
            .header(&Url::parse("http://cas.bjut.edu.cn/login").unwrap())
            .is_none());

        let mut deletion = HeaderMap::new();
        deletion.append(
            SET_COOKIE,
            "host=still-nonempty; Path=/; Max-Age=0".parse().unwrap(),
        );
        jar.absorb(&url, &deletion);
        assert!(jar
            .header(&Url::parse("https://cas.bjut.edu.cn/login").unwrap())
            .is_none());
    }
}
