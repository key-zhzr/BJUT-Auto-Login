pub(crate) fn build_timestamp() -> u64 {
    env!("BJUT_BUILD_TIMESTAMP")
        .parse()
        .expect("build script provides a valid timestamp")
}

#[cfg(any(desktop, test))]
pub(crate) fn is_newer_release(
    version_order: std::cmp::Ordering,
    installed_timestamp: u64,
    remote_timestamp: Option<u64>,
) -> bool {
    version_order.is_lt()
        || (version_order.is_eq()
            && remote_timestamp
                .is_some_and(|stamp| stamp > installed_timestamp && stamp <= 253_402_300_799))
}

fn content_size(status: reqwest::StatusCode, headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let value = if status == reqwest::StatusCode::PARTIAL_CONTENT {
        let range = headers.get(reqwest::header::CONTENT_RANGE)?.to_str().ok()?;
        range.strip_prefix("bytes ")?.rsplit_once('/')?.1
    } else if status.is_success() {
        headers
            .get(reqwest::header::CONTENT_LENGTH)?
            .to_str()
            .ok()?
    } else {
        return None;
    };
    value
        .parse::<u64>()
        .ok()
        .filter(|size| *size > 0 && *size <= 9_007_199_254_740_991)
}

pub(crate) async fn asset_size(url: String) -> Result<Option<u64>, String> {
    let url = reqwest::Url::parse(&url).map_err(|_| "下载地址无效")?;
    if !crate::is_official_github_release_download(&url) {
        return Err("仅支持本项目的官方发布文件".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.stop();
            }
            if attempt.url().scheme() == "https"
                && matches!(
                    attempt.url().host_str(),
                    Some(
                        "github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                    )
                )
            {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(crate::redact_request_error)?;
    Ok(
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            if let Ok(response) = client.head(url.clone()).send().await {
                if let Some(size) = content_size(response.status(), response.headers()) {
                    return Some(size);
                }
            }
            // A one-byte range is a metadata fallback for CDNs that reject HEAD.
            // Drop the response after its headers, even if the server ignores Range.
            let response = client
                .get(url)
                .header(reqwest::header::RANGE, "bytes=0-0")
                .send()
                .await
                .ok()?;
            content_size(response.status(), response.headers())
        })
        .await
        .ok()
        .flatten(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_newer_builds_can_update_the_same_version() {
        use std::cmp::Ordering::{Equal, Greater, Less};
        let installed = 1_794_931_200;
        assert!(build_timestamp() > 0);
        assert!(is_newer_release(Less, installed, None));
        assert!(is_newer_release(Less, installed, Some(installed - 1)));
        assert!(is_newer_release(Equal, installed, Some(installed + 1)));
        assert!(!is_newer_release(Equal, installed, Some(installed)));
        assert!(!is_newer_release(Equal, installed, Some(installed - 1)));
        assert!(!is_newer_release(Equal, installed, None));
        assert!(!is_newer_release(Equal, installed, Some(u64::MAX)));
        assert!(!is_newer_release(Greater, installed, Some(installed + 1)));
    }

    #[test]
    fn range_metadata_reports_the_full_file_not_the_one_byte_response() {
        use reqwest::{header::*, StatusCode};
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("1"));
        headers.insert(
            CONTENT_RANGE,
            HeaderValue::from_static("bytes 0-0/12345678"),
        );
        assert_eq!(
            content_size(StatusCode::PARTIAL_CONTENT, &headers),
            Some(12_345_678)
        );
        headers.insert(CONTENT_RANGE, HeaderValue::from_static("bytes 0-0/*"));
        assert_eq!(content_size(StatusCode::PARTIAL_CONTENT, &headers), None);
        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("10485760"));
        assert_eq!(content_size(StatusCode::OK, &headers), Some(10_485_760));
        assert_eq!(content_size(StatusCode::FORBIDDEN, &headers), None);
    }
}
