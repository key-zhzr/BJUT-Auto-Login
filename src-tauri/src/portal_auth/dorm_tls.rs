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
pub(super) const TLS_HOST_CANDIDATES: [&str; 2] = ["wlgn.bjut.edu.cn", "lgn.bjut.edu.cn"];

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
}

impl DormCertificateVerifier {
    fn pinned_leaf_matches(end_entity: &CertificateDer<'_>) -> bool {
        let fingerprint: [u8; 32] = Sha256::digest(end_entity.as_ref()).into();
        fingerprint == EXPIRED_DORM_CERT_SHA256
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
    /// 叶子证书逐字节匹配已知 pin、SNI 在校园网白名单内。任何一项不满足
    /// 都维持标准校验结论——换发的有效证书走正常 PKI 直接通过；错误主机名、
    /// 陌生证书、无效链等一律原样拒绝。
    fn pin_exception_applies(
        error: &Error,
        end_entity: &CertificateDer<'_>,
        server_name: &ServerName<'_>,
    ) -> bool {
        Self::is_expiry_error(error)
            && Self::pinned_leaf_matches(end_entity)
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
            Err(error) if Self::pin_exception_applies(&error, end_entity, server_name) => {
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

pub(super) fn client_config() -> Result<ClientConfig, String> {
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let standard = WebPkiServerVerifier::builder(Arc::new(roots))
        .build()
        .map_err(|error| format!("宿舍网 TLS 校验器初始化失败：{error}"))?;
    Ok(ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(DormCertificateVerifier { standard }))
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
}
