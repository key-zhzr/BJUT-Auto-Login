//! Credential-free system-path connectivity checks.
use crate::network_probe::NETWORK_PROBE_TIMEOUT;
#[cfg(target_os = "android")]
use crate::usable_physical_ipv4;

#[derive(Clone, Debug)]
pub(crate) struct InternetProbeOutcome {
    pub(crate) label: &'static str,
    pub(crate) success: bool,
    pub(crate) detail: String,
}

#[cfg(target_os = "android")]
async fn probe_internet_targets(
    source_ip: Option<&str>,
    stop_on_success: bool,
) -> Vec<InternetProbeOutcome> {
    probe_internal(source_ip, stop_on_success, |_| {}).await
}

pub(crate) async fn probe_with_updates(
    source_ip: Option<&str>,
    update: impl FnMut(InternetProbeOutcome),
) -> Vec<InternetProbeOutcome> {
    probe_internal(source_ip, false, update).await
}

async fn probe_internal(
    source_ip: Option<&str>,
    stop_on_success: bool,
    mut update: impl FnMut(InternetProbeOutcome),
) -> Vec<InternetProbeOutcome> {
    let builder = reqwest::Client::builder()
        .timeout(NETWORK_PROBE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .use_rustls_tls();
    #[cfg(target_os = "android")]
    let builder = if let Some(candidate) = source_ip {
        let Some(source) = usable_physical_ipv4(candidate) else {
            return Vec::new();
        };
        builder.local_address(std::net::IpAddr::V4(source))
    } else {
        builder
    };
    #[cfg(not(target_os = "android"))]
    {
        // Desktop proxy/TUN clients often publish synthetic DNS addresses and
        // require public traffic to follow the system's default route. Binding
        // these probes to the physical Wi-Fi/Ethernet source address bypasses
        // that route and falsely reports offline. The physical address remains
        // available separately for campus identity and authentication URLs.
        let _ = source_ip;
    }
    let client = match builder.build() {
        Ok(client) => client,
        Err(_) => return Vec::new(),
    };
    let targets = [
        (
            "Google generate_204",
            "https://connectivitycheck.gstatic.com/generate_204",
            0u8,
        ),
        (
            "Cloudflare generate_204",
            "https://cp.cloudflare.com/generate_204",
            0u8,
        ),
        (
            "Apple Captive Portal",
            "http://captive.apple.com/hotspot-detect.html",
            1u8,
        ),
        (
            "Microsoft Connect Test",
            "http://www.msftconnecttest.com/connecttest.txt",
            2u8,
        ),
    ];
    let checks = targets.into_iter().map(|(label, url, validation)| {
        let client = client.clone();
        async move {
            let response = match client
                .get(url)
                .header("Cache-Control", "no-cache, no-store")
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    return InternetProbeOutcome {
                        label,
                        success: false,
                        detail: format!("请求失败：{error}"),
                    }
                }
            };
            let status = response.status();
            let success = match validation {
                0 => status == reqwest::StatusCode::NO_CONTENT,
                1 if response.status().is_success() => response
                    .text()
                    .await
                    .is_ok_and(|text| text.contains("Success")),
                2 if response.status().is_success() => response
                    .text()
                    .await
                    .is_ok_and(|text| text.trim() == "Microsoft Connect Test"),
                _ => false,
            };
            InternetProbeOutcome {
                label,
                success,
                detail: format!(
                    "HTTP {}，{}",
                    status.as_u16(),
                    if success {
                        "校验通过"
                    } else {
                        "响应不符合预期"
                    }
                ),
            }
        }
    });
    use futures_util::StreamExt;
    let mut checks: futures_util::stream::FuturesUnordered<_> = checks.collect();
    let mut outcomes = Vec::new();
    while let Some(outcome) = checks.next().await {
        let success = outcome.success;
        update(outcome.clone());
        outcomes.push(outcome);
        if stop_on_success && success {
            break;
        }
    }
    outcomes
}

#[cfg(target_os = "android")]
pub(crate) async fn check_internet_from_source(source_ip: Option<&str>) -> bool {
    probe_internet_targets(source_ip, true)
        .await
        .into_iter()
        .any(|result| result.success)
}
