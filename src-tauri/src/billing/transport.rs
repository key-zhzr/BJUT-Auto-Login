//! Keep a single, validated logical origin for cookies and redirects. Only the
//! explicitly selected Maximum mode changes the wire transport to campus HTTP.
use super::*;

#[derive(Clone)]
pub(super) struct BillingClient {
    pub client: Client,
    pub compatibility: VpnCompatibility,
}

impl BillingClient {
    pub fn get(&self, url: Url) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::GET, url)
    }

    pub fn post(&self, url: Url) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::POST, url)
    }

    fn request(&self, method: reqwest::Method, url: Url) -> reqwest::RequestBuilder {
        let host = url.host_str().unwrap_or_default().to_string();
        let wire = wire_url(&url, self.compatibility);
        let mut request = self.client.request(method, wire.clone());
        if wire != url {
            let authority = if host == LGN_HOST {
                format!("{host}:801")
            } else {
                host
            };
            request = request.header(reqwest::header::HOST, authority);
        }
        request
    }

    pub fn origin(&self) -> &'static str {
        if self.compatibility == VpnCompatibility::Maximum {
            "http://jfself.bjut.edu.cn"
        } else {
            BILLING_ORIGIN
        }
    }

    pub fn canonical(&self, url: Url) -> Url {
        canonical_url(url, self.compatibility)
    }

    pub fn referer(&self, url: &Url) -> String {
        let mut value = wire_url(url, self.compatibility);
        // The Host header retains the site's virtual host in direct mode.
        if value != *url {
            let _ = value.set_host(url.host_str());
        }
        value.to_string()
    }
}

fn wire_url(url: &Url, mode: VpnCompatibility) -> Url {
    let mut wire = url.clone();
    if mode != VpnCompatibility::Maximum || url.scheme() != "https" {
        return wire;
    }
    let destination = match (url.host_str(), url.port_or_known_default()) {
        (Some(BILLING_HOST), Some(443)) => ("172.21.0.16", 80),
        (Some(LGN_HOST), Some(ACCOUNT_DISCOVERY_PORT)) => ("172.30.201.2", 801),
        _ => return wire,
    };
    let _ = wire.set_scheme("http");
    let _ = wire.set_host(Some(destination.0));
    let _ = wire.set_port(Some(destination.1));
    wire
}

pub(super) fn canonical_url(mut url: Url, mode: VpnCompatibility) -> Url {
    if mode != VpnCompatibility::Maximum || url.scheme() != "http" {
        return url;
    }
    let origin = match (url.host_str(), url.port_or_known_default()) {
        (Some(BILLING_HOST | "172.21.0.16"), Some(80)) => (BILLING_HOST, 443),
        (Some(LGN_HOST | "172.30.201.2" | "172.30.201.10"), Some(801)) => {
            (LGN_HOST, ACCOUNT_DISCOVERY_PORT)
        }
        _ => return url,
    };
    let _ = url.set_scheme("https");
    let _ = url.set_host(Some(origin.0));
    let _ = url.set_port(Some(origin.1));
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_maximum_uses_the_billing_http_ip_and_preserves_path_and_query() {
        let url = Url::parse("https://jfself.bjut.edu.cn/Self/dashboard?q=1").unwrap();
        for mode in [
            VpnCompatibility::Minimum,
            VpnCompatibility::Low,
            VpnCompatibility::High,
        ] {
            assert_eq!(wire_url(&url, mode), url);
        }
        let wire = wire_url(&url, VpnCompatibility::Maximum);
        assert_eq!(wire.as_str(), "http://172.21.0.16/Self/dashboard?q=1");
        assert_eq!(canonical_url(wire, VpnCompatibility::Maximum), url);
    }

    #[test]
    fn direct_mode_does_not_trust_arbitrary_redirect_hosts_or_ports() {
        for value in [
            "http://evil.example/Self/dashboard",
            "http://172.21.0.16:8080/Self/dashboard",
            "http://jfself.bjut.edu.cn.evil.example/Self/dashboard",
        ] {
            assert!(validate_same_origin(&canonical_url(
                Url::parse(value).unwrap(),
                VpnCompatibility::Maximum
            ))
            .is_err());
        }
        let http = Url::parse("http://jfself.bjut.edu.cn/Self/dashboard").unwrap();
        assert!(
            validate_same_origin(&canonical_url(http.clone(), VpnCompatibility::High)).is_err()
        );
        assert!(validate_same_origin(&canonical_url(http, VpnCompatibility::Maximum)).is_ok());
    }
}
