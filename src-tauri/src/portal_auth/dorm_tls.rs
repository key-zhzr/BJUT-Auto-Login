use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;

/// Official BJUT hostnames already present in captured portal traffic.  They
/// are used as TLS SNI/Host aliases while DNS is pinned to the dorm gateway.
pub(super) use super::endpoints::TLS_HOST_CANDIDATES;

/// 宿舍网关证书指纹。类型上没有“关闭”状态：pin 例外永远存在，
/// 只允许把内置指纹替换成另一张证书的指纹。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DormCertPin {
    /// 内置：2024-08-08～2025-09-09 那张过期证书。默认值。
    #[default]
    BuiltIn,
    /// 用户替换的指纹（32 字节原始 SHA-256）。
    Custom([u8; 32]),
}

impl DormCertPin {
    /// 解析配置字符串。空 = BuiltIn；非法格式在保存时已被拒绝，
    /// 这里再非法就防御性回落到 BuiltIn（不 panic、不放宽）。
    pub(crate) fn from_config(value: &str) -> Self {
        match parse_fingerprint(value) {
            Some(fingerprint) => Self::Custom(fingerprint),
            None => Self::BuiltIn,
        }
    }

    /// 规范化后的指纹（BuiltIn 返回内置常量）。
    fn sha256(&self) -> [u8; 32] {
        match self {
            Self::BuiltIn => EXPIRED_DORM_CERT_SHA256,
            Self::Custom(fingerprint) => *fingerprint,
        }
    }

    /// 配置字符串的校验入口，供 save_config / validate_imported_config 调用。
    pub(crate) fn validate(value: &str) -> Result<(), String> {
        if value.is_empty() {
            return Ok(());
        }
        if value.trim() != value {
            return Err("证书指纹前后不能有空格".to_string());
        }
        if parse_fingerprint(value).is_none() {
            return Err("证书指纹必须是 64 位十六进制字符（可带冒号或短横线分隔）".to_string());
        }
        Ok(())
    }
}

/// Accept the three shapes users copy from browsers/openssl: continuous hex,
/// colon-separated, or dash-separated. Case-insensitive; stored as raw bytes.
fn parse_fingerprint(value: &str) -> Option<[u8; 32]> {
    let normalized: String = value
        .trim()
        .chars()
        .filter(|ch| *ch != ':' && *ch != '-')
        .collect();
    if normalized.len() != 64 || !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let mut fingerprint = [0u8; 32];
    for (index, byte) in fingerprint.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&normalized[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(fingerprint)
}

/// SHA-256 fingerprint of the leaf certificate supplied by 10.21.221.98:802:
/// CN=*.bjut.edu.cn, SAN=*.bjut.edu.cn/bjut.edu.cn,
/// valid 2024-08-08 through 2025-09-09.
const EXPIRED_DORM_CERT_SHA256: [u8; 32] = [
    0xb1, 0x73, 0x63, 0x52, 0x3a, 0x89, 0x9d, 0xc4, 0xb6, 0x97, 0x70, 0xe6, 0xb9, 0x1a, 0xc0, 0x51,
    0x66, 0xd7, 0x51, 0xed, 0x22, 0x55, 0x8b, 0xfb, 0x61, 0x9f, 0xe9, 0xa7, 0x3b, 0x5f, 0x18, 0x0a,
];

/// A point inside the pinned leaf certificate's validity period.  This is
/// used only after normal validation failed specifically because of expiry,
/// and only when the exact leaf fingerprint and allowlisted SNI both match.
const PINNED_VALIDATION_TIME: UnixTime =
    UnixTime::since_unix_epoch(Duration::from_secs(1_755_648_000));

#[derive(Debug)]
struct DormCertificateVerifier {
    standard: Arc<WebPkiServerVerifier>,
    pin: DormCertPin,
}

impl DormCertificateVerifier {
    #[cfg(test)]
    fn pinned_leaf_matches(end_entity: &CertificateDer<'_>) -> bool {
        Self::pinned_leaf_matches_with(DormCertPin::BuiltIn, end_entity)
    }

    fn pinned_leaf_matches_with(pin: DormCertPin, end_entity: &CertificateDer<'_>) -> bool {
        let fingerprint: [u8; 32] = Sha256::digest(end_entity.as_ref()).into();
        fingerprint == pin.sha256()
    }

    fn pinned_name_matches(server_name: &ServerName<'_>) -> bool {
        match server_name {
            ServerName::DnsName(name) => TLS_HOST_CANDIDATES.contains(&name.as_ref()),
            _ => false,
        }
    }

    fn is_expiry_error(error: &Error) -> bool {
        matches!(
            error,
            Error::InvalidCertificate(
                CertificateError::Expired | CertificateError::ExpiredContext { .. }
            )
        )
    }

    /// 三项条件同时满足才允许用历史时间戳重新校验：错误类型恰为证书过期、
    /// 叶子证书逐字节匹配当前 pin、SNI 在校园网白名单内。任何一项不满足
    /// 都维持标准校验结论——换发的有效证书走正常 PKI 直接通过；错误主机名、
    /// 陌生证书、无效链等一律原样拒绝。
    #[cfg(test)]
    fn pin_exception_applies(
        error: &Error,
        end_entity: &CertificateDer<'_>,
        server_name: &ServerName<'_>,
    ) -> bool {
        Self::pin_exception_applies_with(&DormCertPin::BuiltIn, error, end_entity, server_name)
    }

    fn pin_exception_applies_with(
        pin: &DormCertPin,
        error: &Error,
        end_entity: &CertificateDer<'_>,
        server_name: &ServerName<'_>,
    ) -> bool {
        Self::is_expiry_error(error)
            && Self::pinned_leaf_matches_with(*pin, end_entity)
            && Self::pinned_name_matches(server_name)
    }
}

impl ServerCertVerifier for DormCertificateVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        match self.standard.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            Ok(verified) => Ok(verified),
            Err(error)
                if Self::pin_exception_applies_with(&self.pin, &error, end_entity, server_name) =>
            {
                // Re-run the complete chain and hostname verification at a
                // historical in-validity timestamp. This relaxes expiry only;
                // an invalid chain, hostname, encoding or signature still
                // fails, and the TLS handshake proof is verified below.
                self.standard.verify_server_cert(
                    end_entity,
                    intermediates,
                    server_name,
                    ocsp_response,
                    PINNED_VALIDATION_TIME,
                )
            }
            Err(error) => Err(error),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.standard.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.standard.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.standard.supported_verify_schemes()
    }
}

pub(super) fn client_config(pin: DormCertPin) -> Result<ClientConfig, String> {
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let standard = WebPkiServerVerifier::builder(Arc::new(roots))
        .build()
        .map_err(|error| format!("宿舍网 TLS 校验器初始化失败：{error}"))?;
    Ok(ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(DormCertificateVerifier { standard, pin }))
        .with_no_client_auth())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::pki_types::pem::PemObject;

    #[test]
    fn expiry_is_the_only_relaxed_certificate_error() {
        assert!(DormCertificateVerifier::is_expiry_error(
            &Error::InvalidCertificate(CertificateError::Expired)
        ));
        assert!(!DormCertificateVerifier::is_expiry_error(
            &Error::InvalidCertificate(CertificateError::NotValidForName)
        ));
        assert!(!DormCertificateVerifier::is_expiry_error(
            &Error::InvalidCertificate(CertificateError::BadSignature)
        ));
    }

    #[test]
    fn pin_is_limited_to_the_known_bjut_sni_aliases() {
        for host in TLS_HOST_CANDIDATES {
            let name = ServerName::try_from(host).unwrap();
            assert!(DormCertificateVerifier::pinned_name_matches(&name));
        }
        let unrelated = ServerName::try_from("example.com").unwrap();
        assert!(!DormCertificateVerifier::pinned_name_matches(&unrelated));
    }

    #[test]
    fn captured_gateway_certificate_matches_the_embedded_pin() {
        let certificate =
            CertificateDer::from_pem_slice(include_bytes!("fixtures/dorm-gateway-2024.pem"))
                .unwrap();
        assert!(DormCertificateVerifier::pinned_leaf_matches(&certificate));
    }

    fn fixture_leaf() -> CertificateDer<'static> {
        CertificateDer::from_pem_slice(include_bytes!("fixtures/dorm-gateway-2024.pem")).unwrap()
    }

    fn fixture_fingerprint() -> [u8; 32] {
        Sha256::digest(fixture_leaf().as_ref()).into()
    }

    fn fingerprint_hex(fingerprint: [u8; 32]) -> String {
        fingerprint
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn verifier_pair() -> (DormCertificateVerifier, Arc<WebPkiServerVerifier>) {
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let standard = WebPkiServerVerifier::builder(Arc::new(roots))
            .build()
            .unwrap();
        (
            DormCertificateVerifier {
                standard: standard.clone(),
                pin: DormCertPin::BuiltIn,
            },
            standard,
        )
    }

    #[test]
    fn pin_exception_requires_expiry_exact_leaf_and_allowlisted_name() {
        let certificate = fixture_leaf();
        let allowed = ServerName::try_from(TLS_HOST_CANDIDATES[1]).unwrap();
        let expired = Error::InvalidCertificate(CertificateError::Expired);
        assert!(DormCertificateVerifier::pin_exception_applies(
            &expired,
            &certificate,
            &allowed
        ));

        let renamed = ServerName::try_from("portal.example.com").unwrap();
        assert!(!DormCertificateVerifier::pin_exception_applies(
            &expired,
            &certificate,
            &renamed
        ));

        let not_expiry = Error::InvalidCertificate(CertificateError::NotValidForName);
        assert!(!DormCertificateVerifier::pin_exception_applies(
            &not_expiry,
            &certificate,
            &allowed
        ));
    }

    #[test]
    fn wrong_hostname_is_rejected_even_with_the_pinned_leaf() {
        // 错误主机名（含形似的子域仿冒）不是过期错误，pin 例外不得触发。
        assert!(!DormCertificateVerifier::pin_exception_applies(
            &Error::InvalidCertificate(CertificateError::NotValidForName),
            &fixture_leaf(),
            &ServerName::try_from("wlgn.bjut.edu.cn.evil.cn").unwrap(),
        ));
    }

    #[test]
    fn a_different_expired_certificate_is_not_pinned() {
        // 网关若挂出另一张过期证书（字节与 pin 不一致），不受例外保护。
        let mut bytes = fixture_leaf().as_ref().to_vec();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        let different = CertificateDer::from(bytes);
        assert!(!DormCertificateVerifier::pinned_leaf_matches(&different));
        let allowed = ServerName::try_from(TLS_HOST_CANDIDATES[0]).unwrap();
        let expired = Error::InvalidCertificate(CertificateError::Expired);
        assert!(!DormCertificateVerifier::pin_exception_applies(
            &expired, &different, &allowed
        ));
    }

    #[test]
    fn ip_address_sni_is_never_pinned() {
        let name = ServerName::from(std::net::IpAddr::from([10, 21, 221, 98]));
        assert!(!DormCertificateVerifier::pinned_name_matches(&name));
    }

    #[test]
    fn standard_outcomes_pass_through_for_non_expiry_failures() {
        // e2e 对比：非过期失败时，包装器与标准校验器结论完全一致（PKI 优先）。
        let (verifier, standard) = verifier_pair();
        let certificate = fixture_leaf();
        let now = UnixTime::since_unix_epoch(Duration::from_secs(1_790_000_000));

        for host in ["portal.example.com", "wlgn.bjut.edu.cn.evil.cn"] {
            let name = ServerName::try_from(host).unwrap();
            let dorm = verifier.verify_server_cert(&certificate, &[], &name, &[], now);
            let expected = standard.verify_server_cert(&certificate, &[], &name, &[], now);
            assert!(expected.is_err(), "标准校验器应拒绝 {host}");
            assert_eq!(
                format!("{dorm:?}"),
                format!("{expected:?}"),
                "主机名 {host} 的结论必须与标准校验器一致"
            );
        }

        let garbage = CertificateDer::from(vec![0u8; 32]);
        let name = ServerName::try_from(TLS_HOST_CANDIDATES[0]).unwrap();
        let dorm = verifier.verify_server_cert(&garbage, &[], &name, &[], now);
        let expected = standard.verify_server_cert(&garbage, &[], &name, &[], now);
        assert!(expected.is_err());
        assert_eq!(format!("{dorm:?}"), format!("{expected:?}"));
    }

    #[test]
    fn expired_pinned_leaf_bridges_only_expiry() {
        // 网关自 2025-09-09 起提供这张过期证书：标准校验必须失败；例外仅当
        // 标准结论恰为过期时生效，且重校验结论与标准校验器在 pin 时间戳下
        // 的结论一致。若链校验先于有效期失败（未知签发者），例外不得触发。
        let (verifier, standard) = verifier_pair();
        let certificate = fixture_leaf();
        let name = ServerName::try_from(TLS_HOST_CANDIDATES[0]).unwrap();
        let now = UnixTime::since_unix_epoch(Duration::from_secs(1_790_000_000));

        let dorm_now = verifier.verify_server_cert(&certificate, &[], &name, &[], now);
        let standard_now = standard.verify_server_cert(&certificate, &[], &name, &[], now);
        let standard_error = standard_now.as_ref().expect_err("过期证书必须校验失败");
        if DormCertificateVerifier::is_expiry_error(standard_error) {
            let bridged =
                standard.verify_server_cert(&certificate, &[], &name, &[], PINNED_VALIDATION_TIME);
            assert_eq!(format!("{dorm_now:?}"), format!("{bridged:?}"));
        } else {
            assert_eq!(format!("{dorm_now:?}"), format!("{standard_now:?}"));
        }
    }

    #[test]
    fn custom_pin_replaces_the_built_in_fingerprint() {
        let certificate = fixture_leaf();
        let fingerprint = fixture_fingerprint();
        let custom = DormCertPin::Custom(fingerprint);
        assert!(DormCertificateVerifier::pinned_leaf_matches_with(
            DormCertPin::BuiltIn,
            &certificate
        ));
        assert!(DormCertificateVerifier::pinned_leaf_matches_with(
            custom,
            &certificate
        ));
        assert_eq!(custom.sha256(), fingerprint);
    }

    #[test]
    fn custom_pin_does_not_accept_the_built_in_certificate() {
        // 替换而非追加：自定义指纹只认自己的证书，内置证书不再被接受。
        let custom = DormCertPin::Custom([0x22; 32]);
        assert!(!DormCertificateVerifier::pinned_leaf_matches_with(
            custom,
            &fixture_leaf()
        ));
    }

    #[test]
    fn unknown_fingerprint_is_rejected() {
        let custom = DormCertPin::Custom([0x00; 32]);
        assert!(!DormCertificateVerifier::pinned_leaf_matches_with(
            custom,
            &fixture_leaf()
        ));
    }

    #[test]
    fn pin_exception_with_custom_fingerprint_still_requires_expiry_and_allowlisted_name() {
        let certificate = fixture_leaf();
        let pin = DormCertPin::Custom(fixture_fingerprint());
        let allowed = ServerName::try_from(TLS_HOST_CANDIDATES[0]).unwrap();
        let expired = Error::InvalidCertificate(CertificateError::Expired);
        assert!(DormCertificateVerifier::pin_exception_applies_with(
            &pin,
            &expired,
            &certificate,
            &allowed
        ));
        assert!(!DormCertificateVerifier::pin_exception_applies_with(
            &pin,
            &Error::InvalidCertificate(CertificateError::NotValidForName),
            &certificate,
            &allowed
        ));
        let renamed = ServerName::try_from("portal.example.com").unwrap();
        assert!(!DormCertificateVerifier::pin_exception_applies_with(
            &pin,
            &expired,
            &certificate,
            &renamed
        ));
    }

    #[test]
    fn fingerprint_validation_accepts_colons_dashes_and_case() {
        let fingerprint = fixture_fingerprint();
        let continuous = fingerprint_hex(fingerprint);
        let colon = continuous
            .as_bytes()
            .chunks(2)
            .map(|chunk| std::str::from_utf8(chunk).unwrap())
            .collect::<Vec<_>>()
            .join(":");
        let dash = colon.replace(':', "-").to_ascii_uppercase();
        for value in [continuous.as_str(), colon.as_str(), dash.as_str()] {
            assert!(DormCertPin::validate(value).is_ok(), "{value}");
            assert_eq!(
                DormCertPin::from_config(value),
                DormCertPin::Custom(fingerprint),
                "{value}"
            );
        }
    }

    #[test]
    fn fingerprint_validation_rejects_malformed_values() {
        let values = vec![
            "abc".to_string(),
            "zz".repeat(32),
            format!(" {} ", fingerprint_hex(fixture_fingerprint())),
            "a".repeat(63),
            "a".repeat(65),
        ];
        for value in &values {
            assert!(DormCertPin::validate(value).is_err(), "{value}");
        }
    }

    #[test]
    fn empty_fingerprint_keeps_the_built_in_pin() {
        assert!(DormCertPin::validate("").is_ok());
        assert_eq!(DormCertPin::from_config(""), DormCertPin::BuiltIn);
        assert_eq!(DormCertPin::BuiltIn.sha256(), EXPIRED_DORM_CERT_SHA256);
    }
}
