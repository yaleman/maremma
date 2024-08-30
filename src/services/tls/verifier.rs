use super::TlsPeerState;
use crate::prelude::*;
use rustls::client::{verify_server_cert_signed_by_trust_anchor, verify_server_name};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::server::ParsedCertificate;
use rustls::{RootCertStore, SignatureScheme};
use x509_parser::parse_x509_certificate;

#[derive(Debug, Default)]
pub(crate) struct TlsCertVerifier;

impl rustls::client::danger::ServerCertVerifier for TlsCertVerifier {
    #[instrument(level = "debug", skip_all, fields(server_name))]
    /// This is ALWAYS going to throw a [rustls::Error] error, because we don't have a way to pass state back out
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        // parse the end cert
        let (_, cert) = parse_x509_certificate(end_entity.as_ref()).map_err(|err| {
            error!("Failed to parse TLS certificate {:?}", err);
            rustls::Error::General("{}".to_string())
        })?;

        // let this just fail out if it fails because well, too bad
        let parsed_cert = ParsedCertificate::try_from(end_entity)
            .inspect_err(|err| error!("Couldn't parse certificate! {:?}", err))?;

        let mut tls_peer_state = TlsPeerState::new(DateTime::from_timestamp_nanos(
            cert.validity()
                .not_after
                .to_datetime()
                .unix_timestamp_nanos() as i64,
        ));

        tls_peer_state.cert_name_matches = verify_server_name(&parsed_cert, server_name).is_ok();

        let root_store = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.into(),
        };

        for intermediate in intermediates {
            if let Ok(parsed_intermediate) = ParsedCertificate::try_from(intermediate) {
                if verify_server_cert_signed_by_trust_anchor(
                    &parsed_intermediate,
                    &root_store,
                    &[],
                    UnixTime::now(),
                    &[],
                )
                .is_err()
                {
                    tls_peer_state.set_intermediate_untrusted();
                    debug!("Intermediate is untrusted");
                }
            }

            if let Ok((_, cert)) = parse_x509_certificate(intermediate.as_ref()) {
                if !cert.validity.is_valid() {
                    tls_peer_state.set_intermediate_expired();
                }

                // TODO: match cert signing algo
            }
        }

        Err(rustls::Error::General(
            #[cfg(not(tarpaulin_include))]
            // We're unlikely to hit this, so testing it's not really helpful.
            serde_json::to_string(&tls_peer_state).map_err(|err| {
                error!("Failed to serialize TLS state {:?}", err);
                rustls::Error::General("{}".to_string())
            })?,
        ))
    }

    #[instrument(level = "debug")]
    /// This is ALWAYS going to throw a [rustls::Error::General] error if we ever get to it, because we don't have a way to pass state back out
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General(
            "verify_tls12_signature is unimplemented, but you shouldn't get here anyway!"
                .to_string(),
        ))
    }

    #[instrument(level = "debug")]
    /// This is ALWAYS going to throw a [rustls::Error] error, because we don't have a way to pass state back out
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General(
            "verify_tls13_signature is unimplemented, but you shouldn't get here anyway!"
                .to_string(),
        ))
    }

    #[instrument(level = "debug")]
    /// We really do support everything, we're nice like that.
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.all_signature_schemes()
    }
}

impl TlsCertVerifier {
    /// Returns all the possible schemes
    fn all_signature_schemes(&self) -> Vec<SignatureScheme> {
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
