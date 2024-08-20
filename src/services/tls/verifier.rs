use super::TlsPeerState;
use crate::prelude::*;
use rustls::client::verify_server_name;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::server::ParsedCertificate;
use rustls::SignatureScheme;

#[derive(Debug, Default)]
pub(crate) struct TlsCertVerifier;

impl rustls::client::danger::ServerCertVerifier for TlsCertVerifier {
    #[instrument(level = "debug", skip(end_entity))]
    /// This is ALWAYS going to throw a [rustlts::Error::General] error, because we don't have a way to pass state back out
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        // parse the end cert
        use x509_parser::parse_x509_certificate;
        let (_, cert) = parse_x509_certificate(end_entity.as_ref()).unwrap();

        // let this just fail out if it fails because well, too bad
        let parsed_cert = ParsedCertificate::try_from(end_entity)?;

        let mut tls_peer_state = TlsPeerState::new(DateTime::from_timestamp_nanos(
            cert.validity()
                .not_after
                .to_datetime()
                .unix_timestamp_nanos() as i64,
        ));

        tls_peer_state.cert_name_matches = verify_server_name(&parsed_cert, server_name).is_ok();

        for intermediate in intermediates {
            if let Ok((_, cert)) = parse_x509_certificate(intermediate.as_ref()) {
                if !cert.validity.is_valid() {
                    tls_peer_state.set_intermediate_expired();
                    break;
                }
            } else {
                debug!("Couldn't parse intermediate certificate... that's odd.")
            }
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
