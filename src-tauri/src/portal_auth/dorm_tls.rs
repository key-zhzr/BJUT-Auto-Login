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
                if Self::is_expiry_error(&error)
                    && Self::pinned_leaf_matches(end_entity)
                    && Self::pinned_name_matches(server_name) =>
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
}
