//! TLS service checks

#[cfg(test)]
mod tests;
pub(crate) mod verifier;

use std::num::NonZeroU16;

use schemars::JsonSchema;
use verifier::TlsCertVerifier;

use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

use super::prelude::*;
use crate::prelude::*;

/// The IO error returns something like this and we want to find it: `IoError("unexpected error: {\"expiry\":\"2024-11-07T15:05:43Z\"}")`
pub const UNEXPECTED_ERROR_PREFIX: &str = "unexpected error: ";

/// Default value for "expires in days" to trigger a critical alert
pub const DEFAULT_CRITICAL_DAYS: u16 = 0;
/// Default value for "expires in days" to trigger a warning alert
pub const DEFAULT_WARNING_DAYS: u16 = 1;

/// For when you want to check TLS things like certificate expiries etc
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
pub struct TlsService {
    // TODO: CA cert
    // TODO: sni/hostname to check
    /// Name of the service
    pub name: String,
    #[serde(with = "crate::serde::cron")]
    #[schemars(with = "String")]
    /// Schedule to run the check on
    pub cron_schedule: Cron,

    /// Port to connect to
    pub port: NonZeroU16,

    /// Critical expiry in days, defaults to [DEFAULT_CRITICAL_DAYS] (0)
    pub expiry_critical: Option<u16>,
    /// Warning expiry in days, defaults to [DEFAULT_WARNING_DAYS] (1)
    pub expiry_warn: Option<u16>,

    /// Defaults to 10 seconds
    pub timeout: Option<u16>,

    /// Add random jitter in 0..n seconds to the check
    pub jitter: Option<u16>,
}

impl ConfigOverlay for TlsService {
    fn overlay_host_config(&self, value: &Map<String, Json>) -> Result<Box<Self>, Error> {
        Ok(Box::new(Self {
            name: self.extract_string(value, "name", &self.name),
            cron_schedule: self.extract_cron(value, "cron_schedule", &self.cron_schedule)?,
            port: self.extract_value(value, "port", &self.port)?,
            expiry_critical: self.extract_value(value, "expiry_critical", &self.expiry_critical)?,
            expiry_warn: self.extract_value(value, "expiry_warn", &self.expiry_warn)?,
            timeout: self.extract_value(value, "timeout", &self.timeout)?,
            jitter: self.extract_value(value, "jitter", &self.jitter)?,
        }))
    }
}

#[async_trait]
impl ServiceTrait for TlsService {
    #[instrument(level = "debug", skip(self), fields(name=self.name, cron=self.cron_schedule.pattern.to_string(),port=self.port,
    expiry_critical=self.expiry_critical,
    expiry_warn=self.expiry_warn,
    timeout=self.timeout))]
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        let start_time = chrono::Utc::now();

        // this comes from the rustls example here: https://github.com/rustls/tokio-rustls/blob/HEAD/examples/client.rs
        let root_store = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.into(),
        };
        let mut client_config: ClientConfig = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        //  we use our own verifier because we want all the data
        let tls_verifier = Arc::new(TlsCertVerifier);
        // nosemgrep: rust.lang.security.rustls-dangerous.rustls-dangerous
        client_config
            .dangerous()
            .set_certificate_verifier(tls_verifier);

        let connector = TlsConnector::from(Arc::new(client_config));
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
                "Something went hinky in the TLS check parser, it should always return an 'Error'!"
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
        let mut result_strings = Vec::new();

        let expiry_critical_seconds =
            self.expiry_critical.unwrap_or(DEFAULT_CRITICAL_DAYS) as i64 * 86400;
        let expiry_warn_seconds = self.expiry_warn.unwrap_or(DEFAULT_WARNING_DAYS) as i64 * 86400;

        if result.cert_expired() {
            status = ServiceStatus::Critical;
            result_strings.push(format!(
                "Certificate expired {} days ago",
                -result.expiry_days()
            ));
        }
        if !result.cert_name_matches {
            status = ServiceStatus::Critical;
            result_strings.push("Certificate name does not match".to_string());
        }
        if result.intermediate_expired {
            status = ServiceStatus::Critical;
            result_strings.push("Intermediate certificate expired".to_string());
        }
        if result.intermediate_untrusted {
            status = ServiceStatus::Critical;
            result_strings.push("Intermediate certificate untrusted".to_string());
        }

        if result.expiry_seconds() <= expiry_critical_seconds {
            status = ServiceStatus::Critical;
            result_strings.push(format!(
                "Certificate expires in {} days or {} seconds - min set to {}",
                result.expiry_days(),
                result.expiry_seconds(),
                expiry_critical_seconds
            ));
        } else if result.expiry_seconds() <= expiry_warn_seconds {
            status = ServiceStatus::Warning;
            result_strings.push(format!(
                "Certificate expires in {} days or {} seconds - min set to {}",
                result.expiry_days(),
                result.expiry_seconds(),
                expiry_warn_seconds
            ));
        }

        let result_text = result_strings.join(", ");
        if result_text.is_empty() {
            result_strings.push("OK".to_string());
        }

        let timestamp = chrono::Utc::now();

        Ok(CheckResult {
            timestamp,
            time_elapsed: timestamp - start_time,
            status,
            result_text,
        })
    }

    fn as_json_pretty(&self, host: &entities::host::Model) -> Result<String, Error> {
        let config = self.overlay_host_config(&self.get_host_config(&self.name, host)?)?;
        Ok(serde_json::to_string_pretty(&config)?)
    }

    fn jitter_value(&self) -> u32 {
        self.jitter.unwrap_or(0) as u32
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub(crate) struct TlsPeerState {
    cert_name_matches: bool,
    end_cert_expiry: DateTime<Utc>,
    intermediate_expired: bool,
    intermediate_untrusted: bool,
    servername: Option<String>,
}

impl TlsPeerState {
    pub fn new(end_cert_expiry: DateTime<Utc>) -> Self {
        Self {
            end_cert_expiry,
            cert_name_matches: false,
            intermediate_expired: false,
            intermediate_untrusted: false,
            servername: None,
        }
    }
    pub fn set_intermediate_expired(&mut self) {
        self.intermediate_expired = true;
    }
    #[allow(dead_code)] // because the verify function is busted
    pub fn set_intermediate_untrusted(&mut self) {
        self.intermediate_untrusted = true;
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
    /// Return the number of seconds until the certificate expires
    pub fn expiry_seconds(&self) -> i64 {
        let now = chrono::Utc::now();
        (self.end_cert_expiry - now).num_seconds()
    }
}
