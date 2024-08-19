#[cfg(test)]
mod tests;
pub(crate) mod verifier;

use verifier::TlsCertVerifier;

use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

use crate::prelude::*;

/// The IO error returns something like this and we want to find it: `IoError("unexpected error: {\"expiry\":\"2024-11-07T15:05:43Z\"}")`
const UNEXPECTED_ERROR_PREFIX: &str = "unexpected error: ";

/// For when you want to check TLS things like certificate expiries etc
#[derive(Serialize, Deserialize, Debug)]
pub struct TlsService {
    pub name: String,
    #[serde(
        deserialize_with = "crate::serde::deserialize_croner_cron",
        serialize_with = "crate::serde::serialize_croner_cron"
    )]
    pub cron_schedule: Cron,

    /// Port to connect to
    pub port: u16,

    /// Critical expiry in days
    pub expiry_critical: Option<u16>,
    /// Warning expiry in days
    pub expiry_warn: Option<u16>,

    /// Defaults to 10 seconds
    pub timeout: Option<u16>,
}

#[async_trait]
impl ServiceTrait for TlsService {
    #[instrument(level = "debug")]
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        if self.port == 0 {
            return Err(Error::InvalidInput("Port cannot be 0".to_string()));
        }
        // this comes from the rustls example here: https://github.com/rustls/tokio-rustls/blob/HEAD/examples/client.rs
        let root_store = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.into(),
        };
        let mut config: ClientConfig = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        //  we use our own verifier because we want all the datas
        let tls_verifier = Arc::new(TlsCertVerifier::new());
        config.dangerous().set_certificate_verifier(tls_verifier);

        let connector = TlsConnector::from(Arc::new(config));
        let dnsname = ServerName::try_from(host.hostname.clone()).map_err(|err| {
            error!("Failed to resolve {} {:?}", host.hostname, err);
            Error::DNSFailed // TODO: this is a valid state, handle it better
        })?;

        let timeout_duration = tokio::time::Duration::from_secs(self.timeout.unwrap_or(10) as u64);
        let stream = match tokio::time::timeout(
            timeout_duration,
            TcpStream::connect(format!("{}:{}", host.hostname, self.port)),
        )
        .await
        {
            Ok(val) => val?,
            Err(_) => return Err(Error::Timeout),
        };

        let result: TlsPeerState = match connector.connect(dnsname, stream).await {
            Ok(_val) => return Err(Error::Generic(
                "Something went hinky in the TLS check parser, it should always return an 'error'!"
                    .to_string(),
            )),

            Err(err) => {
                // This is so bad. I know.
                let err_string = err.to_string();
                if err_string.starts_with(UNEXPECTED_ERROR_PREFIX) {
                    let (_, val) = err_string.split_at(UNEXPECTED_ERROR_PREFIX.len());
                    debug!("Found our sneaky serialized state in the error! {}", val);
                    // let's try and deserialize this thing
                    serde_json::from_str(val)?
                } else {
                    return Err(err.into());
                }
            }
        };

        let status = match result.status {
            TlsPeerStatus::Ok => ServiceStatus::Ok,
            TlsPeerStatus::EndCertExpired => ServiceStatus::Critical,
            TlsPeerStatus::CaCertExpired(_expiry) => ServiceStatus::Critical,
            TlsPeerStatus::Unknown => ServiceStatus::Unknown,
        };
        let result_text = "OK".to_string(); // TODO: update the result text to be more useful

        Ok(CheckResult {
            timestamp: chrono::Utc::now(),
            time_elapsed: TimeDelta::zero(),
            status,
            result_text,
        })
    }
}

#[derive(Deserialize, Serialize, Debug, Default)]
pub(crate) enum TlsPeerStatus {
    Ok,
    #[default]
    Unknown,
    EndCertExpired,
    CaCertExpired(DateTime<Utc>),
}

#[derive(Deserialize, Serialize, Debug, Default)]
pub(crate) struct TlsPeerState {
    pub status: TlsPeerStatus,
    pub expiry_tail: Option<DateTime<Utc>>,
}
