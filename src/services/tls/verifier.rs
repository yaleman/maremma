use super::{TlsPeerState, TlsPeerStatus};
use crate::prelude::*;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::SignatureScheme;

#[derive(Debug, Default)]
pub(crate) struct TlsCertVerifier {
    /// How many days until the certificate expires, if we can validate it
    #[allow(dead_code)]
    pub expiry: Option<DateTime<Utc>>,
}

impl TlsCertVerifier {
    pub fn new() -> Self {
        Self { expiry: None }
    }
    #[allow(dead_code)]
    /// Is the cert expired (or can we )
    pub fn expired(&self) -> bool {
        match self.expiry {
            None => true,
            Some(expiry) => {
                let now = chrono::Utc::now();
                expiry < now
            }
        }
    }
}

impl rustls::client::danger::ServerCertVerifier for TlsCertVerifier {
    #[instrument(level = "debug")]
    /// This is ALWAYS going to throw a [rustlts::Error::General] error, because we don't have a way to pass state back out
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        // parse the end cert
        use x509_parser::parse_x509_certificate;
        let (_, cert) = parse_x509_certificate(end_entity.as_ref()).unwrap();

        let mut tls_peer_state = TlsPeerState::default();

        let expiry: chrono::DateTime<Utc> = DateTime::from_timestamp_nanos(
            cert.validity()
                .not_after
                .to_datetime()
                .unix_timestamp_nanos() as i64,
        );

        tls_peer_state.expiry_tail = Some(expiry);
        if cert.validity().time_to_expiration().is_none() {
            tls_peer_state.status = TlsPeerStatus::EndCertExpired;
        }

        if let TlsPeerStatus::Unknown = tls_peer_state.status {
            tls_peer_state.status = TlsPeerStatus::Ok;
        }

        Err(rustls::Error::General(
            serde_json::to_string(&tls_peer_state).map_err(|err| {
                error!("Failed to serialize TLS state {:?}", err);
                rustls::Error::General("{}".to_string())
            })?,
        ))
    }

    #[instrument(level = "debug")]
    /// This is ALWAYS going to throw a [rustlts::Error::General] error, because we don't have a way to pass state back out
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        todo!()
    }

    #[instrument(level = "debug")]
    /// This is ALWAYS going to throw a [rustlts::Error::General] error, because we don't have a way to pass state back out
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        todo!()
    }

    #[instrument(level = "debug")]
    /// We really do support everything, we're nice like that.
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        // TODO: do we really want to support all of these, or offer a way to limit them
        vec![
            SignatureScheme::RSA_PKCS1_SHA1,
            SignatureScheme::ECDSA_SHA1_Legacy,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
            SignatureScheme::ED448,
        ]
    }
}
