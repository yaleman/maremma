use std::net::ToSocketAddrs;
use std::time::Duration;

use chrono::Utc;
use serde::Deserialize;

use crate::prelude::*;

pub const DEFAULT_SSH_TIMEOUT_SECONDS: u16 = 30;
pub const DEFAULT_SSH_PORT: u16 = 22;

fn default_ssh_port() -> u16 {
    DEFAULT_SSH_PORT
}

fn default_timeout_seconds() -> u16 {
    DEFAULT_SSH_TIMEOUT_SECONDS
}

#[derive(Default, Deserialize, Serialize, Debug)]
pub struct SshHost {
    pub hostname: String,
    /// Defaults to [DEFAULT_SSH_PORT] (22)
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// If you want to connect via IP address instead
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<std::net::IpAddr>,
    /// Defaults to [DEFAULT_SSH_TIMEOUT_SECONDS]
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u16,
    /// If you're not just connecting as "you"
    pub remote_user: Option<String>,

    #[serde(default)]
    pub host_groups: Vec<String>,

    #[serde(default)]
    pub disabled: bool,

    #[serde(skip)]
    pub last_check: Option<DateTime<Utc>>,

    #[serde(skip)]
    pub service_checks: Vec<Box<dyn ServiceTrait>>,
}

impl SshHost {
    pub fn from_hostname(hostname: impl ToString) -> Self {
        Self {
            hostname: hostname.to_string(),
            ..Default::default()
        }
    }
    pub fn with_timeout(self, timeout_seconds: u16) -> Self {
        Self {
            timeout_seconds,
            ..self
        }
    }
}

#[async_trait]
impl GenericHost for SshHost {
    fn id(&self) -> String {
        sha256::digest(&format!(
            "{}@{}:{}",
            self.remote_user.as_ref().unwrap_or(&"user".to_string()),
            self.hostname,
            self.port
        ))
    }
    fn name(&self) -> String {
        format!("SshHost({}:{})", self.hostname, self.port)
    }

    async fn check_up(&self) -> Result<bool, Error> {
        let socket_address = match self.ip_address {
            Some(ip) => match (ip, self.port)
                .to_socket_addrs()
                .map_err(|_err| Error::DNSFailed)?
                .next()
            {
                Some(sock) => sock,
                None => return Err(Error::DNSFailed),
            },
            None => format!("{}:{}", self.hostname, self.port)
                .to_socket_addrs()
                .expect("Failed to resolve hostname")
                .next()
                .expect("Failed to resolve hostname"),
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
        serde_json::from_value(config).map_err(|e| Error::ConfigParse(e.to_string()))
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
        assert_eq!(res.unwrap(), false);
    }

    #[test]
    fn test_config_parse() {
        let config = r#"
            {
                "hostname": "example.com",
                "timeout_seconds": 5
            }
        "#;
        let host: SshHost = serde_json::from_str(config).unwrap();
        assert_eq!(host.hostname, "example.com");
        assert_eq!(host.port, DEFAULT_SSH_PORT);
        assert_eq!(host.timeout_seconds, 5);
    }
}
