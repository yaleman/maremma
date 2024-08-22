//! TLS service checks

#[cfg(test)]
mod tests;
pub(crate) mod verifier;

use schemars::JsonSchema;
use verifier::TlsCertVerifier;

use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

use crate::prelude::*;

/// The IO error returns something like this and we want to find it: `IoError("unexpected error: {\"expiry\":\"2024-11-07T15:05:43Z\"}")`
pub const UNEXPECTED_ERROR_PREFIX: &str = "unexpected error: ";

/// Default value for "expires in days" to trigger a critical alert
pub static DEFAULT_CRITICAL_DAYS: u16 = 0;
/// Default value for "expires in days" to trigger a warning alert
pub static DEFAULT_WARNING_DAYS: u16 = 1;

/// For when you want to check TLS things like certificate expiries etc
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
pub struct TlsService {
    /// Name of the service
    pub name: String,
    #[serde(
        deserialize_with = "crate::serde::deserialize_croner_cron",
        serialize_with = "crate::serde::serialize_croner_cron"
    )]
    #[schemars(with = "String")]
    /// Schedule to run the check on
    pub cron_schedule: Cron,

    /// Port to connect to
    pub port: u16,

    /// Critical expiry in days, defaults to [DEFAULT_CRITICAL_DAYS] (0)
    pub expiry_critical: Option<u16>,
    /// Warning expiry in days, defaults to [DEFAULT_WARNING_DAYS] (1)
    pub expiry_warn: Option<u16>,

    /// Defaults to 10 seconds
    pub timeout: Option<u16>,
    // TODO: CA cert
    // TODO: sni/hostname to check
}

#[async_trait]
impl ServiceTrait for TlsService {
    #[instrument(level = "debug")]
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        let start_time = chrono::Utc::now();
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

        //  we use our own verifier because we want all the data
        let tls_verifier = Arc::new(TlsCertVerifier);
        config.dangerous().set_certificate_verifier(tls_verifier);

        let connector = TlsConnector::from(Arc::new(config));
        let dnsname = match ServerName::try_from(host.hostname.clone()) {
            Ok(val) => val,
            Err(_err) => {
                debug!(
                    "Invalid hostname specified for TLS check hostname={}",
                    host.hostname
                );
                let timestamp = chrono::Utc::now();
                return Ok(CheckResult {
                    time_elapsed: start_time - timestamp,
                    timestamp: chrono::Utc::now(),
                    status: ServiceStatus::Critical,
                    result_text: format!("Invalid hostname '{}'", host.hostname),
                });
            }
        };

        let timeout_duration = tokio::time::Duration::from_secs(self.timeout.unwrap_or(10) as u64);
        let stream = match tokio::time::timeout(
            timeout_duration,
            TcpStream::connect(format!("{}:{}", host.hostname, self.port)),
        )
        .await
        {
            Ok(val) => match val {
                Ok(val) => val,
                Err(err) => {
                    debug!(
                        "Failed to TcpStream::connect to hostname=\"{}\" error=\"{}\"",
                        host.hostname, err
                    );
                    let timestamp = chrono::Utc::now();
                    return Ok(CheckResult {
                        time_elapsed: start_time - timestamp,
                        timestamp: chrono::Utc::now(),
                        status: ServiceStatus::Critical,
                        result_text: format!(
                            "Failed to connect to hostname=\"{}\" error=\"{}\"",
                            host.hostname, err
                        ),
                    });
                }
            },
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

        let mut status = ServiceStatus::Ok;
        let mut result_text = "OK".to_string();
        if result.cert_expired() {
            status = ServiceStatus::Critical;
            result_text = format!("Certificate expired {} days ago", -result.expiry_days());
        } else if !result.cert_name_matches {
            status = ServiceStatus::Critical;
            result_text = "Certificate name does not match".to_string();
        } else if result.intermediate_expired {
            status = ServiceStatus::Critical;
            result_text = "Intermediate certificate expired".to_string();
        } else if result.expiry_days()
            <= self.expiry_critical.unwrap_or(DEFAULT_CRITICAL_DAYS) as i64
        {
            status = ServiceStatus::Critical;
            result_text = format!("Certificate expires in {} days", result.expiry_days());
        } else if result.expiry_days() <= self.expiry_warn.unwrap_or(DEFAULT_WARNING_DAYS) as i64 {
            status = ServiceStatus::Warning;
            result_text = format!("Certificate expires in {} days", result.expiry_days());
        }

        let timestamp = chrono::Utc::now();

        Ok(CheckResult {
            timestamp,
            time_elapsed: timestamp - start_time,
            status,
            result_text,
        })
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub(crate) struct TlsPeerState {
    cert_name_matches: bool,
    end_cert_expiry: DateTime<Utc>,
    intermediate_expired: bool,
    servername: Option<String>,
}

impl TlsPeerState {
    pub fn new(end_cert_expiry: DateTime<Utc>) -> Self {
        Self {
            end_cert_expiry,
            cert_name_matches: false,
            intermediate_expired: false,
            servername: None,
        }
    }
    pub fn set_intermediate_expired(&mut self) {
        self.intermediate_expired = true;
    }

    /// Return if the cert has expired
    pub fn cert_expired(&self) -> bool {
        (self.end_cert_expiry - chrono::Utc::now()).num_seconds() <= 0
    }

    /// Return the number of days until the certificate expires
    pub fn expiry_days(&self) -> i64 {
        let now = chrono::Utc::now();
        (self.end_cert_expiry - now).num_days()
    }
}
