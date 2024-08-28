use std::net::ToSocketAddrs;
use std::num::NonZeroU16;
use std::time::Duration;

use chrono::Utc;
use serde::Deserialize;

use crate::prelude::*;

/// The default timeout
pub const DEFAULT_SSH_TIMEOUT_SECONDS: u16 = 30;
/// Guess?
pub const DEFAULT_SSH_PORT: u16 = 22;

fn default_ssh_port() -> NonZeroU16 {
    #[allow(clippy::expect_used)]
    NonZeroU16::new(DEFAULT_SSH_PORT).expect("Failed to parse 22 as non-zero u16!")
}

fn default_ssh_timeout_seconds() -> u16 {
    DEFAULT_SSH_TIMEOUT_SECONDS
}

#[derive(Default, Deserialize, Serialize, Debug)]
/// An SSH-connected host
pub struct SshHost {
    /// The hostname
    pub hostname: String,
    /// Defaults to [DEFAULT_SSH_PORT] (22)
    pub port: Option<NonZeroU16>,
    /// If you want to connect via IP address instead
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<std::net::IpAddr>,
    /// Defaults to [DEFAULT_SSH_TIMEOUT_SECONDS]
    #[serde(default = "default_ssh_timeout_seconds")]
    pub timeout_seconds: u16,
    /// If you're not just connecting as "you"
    pub remote_user: Option<String>,

    #[serde(default)]
    /// Groups that this host is part of
    pub host_groups: Vec<String>,

    #[serde(default)]
    /// If this host is disabled
    pub disabled: bool,

    #[serde(skip)]
    /// The last time we checked this host
    pub last_check: Option<DateTime<Utc>>,

    #[serde(skip)]
    /// The list of service checks for this host
    pub service_checks: Vec<Box<dyn ServiceTrait>>,
}

impl SshHost {
    /// Create a new SshHost from a hostname
    pub fn from_hostname(hostname: &str) -> Self {
        Self {
            hostname: hostname.to_string(),
            ..Default::default()
        }
    }
    /// Update the timeout
    pub fn with_timeout(self, timeout_seconds: u16) -> Self {
        Self {
            timeout_seconds,
            ..self
        }
    }
}

#[async_trait]
impl GenericHost for SshHost {
    async fn check_up(&self) -> Result<bool, Error> {
        let socket_address = match self.ip_address {
            Some(ip) => match (ip, u16::from(self.port.unwrap_or(default_ssh_port())))
                .to_socket_addrs()
                .map_err(|_err| Error::DnsFailed)?
                .next()
            {
                Some(sock) => sock,
                None => return Err(Error::DnsFailed),
            },
            None => match format!(
                "{}:{}",
                self.hostname,
                self.port.unwrap_or(default_ssh_port())
            )
            .to_socket_addrs()
            .map_err(|_err| Error::DnsFailed)?
            .next()
            {
                Some(val) => val,
                None => return Err(Error::DnsFailed),
            },
        };
        let result = std::net::TcpStream::connect_timeout(
            &socket_address,
            Duration::from_secs(self.timeout_seconds as u64),
        );
        match result {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    fn try_from_config(config: serde_json::Value) -> Result<Self, Error>
    where
        Self: Sized,
    {
        Self::try_from(&config)
    }
}

impl TryFrom<&Value> for SshHost {
    type Error = Error;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value.clone()).map_err(|e| Error::Deserialization(e.to_string()))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_check_up() {
        // check we have a test host defined in the MAREMMA_TEST_SSH_HOST env var
        let hostname = match std::env::var("MAREMMA_TEST_SSH_HOST") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("MAREMMA_TEST_SSH_HOST not set, skipping test");
                return;
            }
        };
        let host = SshHost::from_hostname(&hostname).with_timeout(5);
        assert!(host.check_up().await.is_ok());

        let example_com = SshHost::from_hostname("example.com").with_timeout(1);
        let res = example_com.check_up().await;

        assert!(res.is_ok());
        assert!(!res.unwrap());
    }

    #[test]
    fn test_config_parse() {
        let config = r#"
            {
                "hostname": "example.com",
                "timeout_seconds": 1234
            }
        "#;
        let host: SshHost = serde_json::from_str(config).unwrap();
        assert_eq!(host.hostname, "example.com");
        assert_eq!(host.port, None);
        assert_eq!(host.timeout_seconds, 1234);
    }
    #[test]
    fn test_try_from_value() {
        let config = serde_json::json! {
                {
                    "hostname": "example.com",
                    "port" : 123
                }
        };
        let host = SshHost::try_from(&config).unwrap();
        assert_eq!(host.hostname, "example.com");
        assert_eq!(
            host.port,
            Some(NonZeroU16::new(123).expect("failed to parse 123 as a non-zero u16"))
        );
        assert_eq!(host.timeout_seconds, default_ssh_timeout_seconds());
        assert_eq!(
            SshHost::try_from_config(config).unwrap().hostname,
            host.hostname
        );
    }
}
