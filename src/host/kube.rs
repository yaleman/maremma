use std::num::NonZeroU16;
use std::path::PathBuf;

use kube::Client;

use crate::prelude::*;

/// The default port we'll try and connect to
pub(crate) fn kube_port_default() -> NonZeroU16 {
    #[allow(clippy::expect_used)]
    NonZeroU16::new(6443u16).expect("Failed to parse kube_port_default")
}

#[derive(Deserialize, Serialize, Debug)]
/// A kubernetes host
pub struct KubeHost {
    /// Target hostname
    pub hostname: String,
    /// Defaults to 6443
    #[serde(default = "kube_port_default")]
    pub api_port: NonZeroU16,
    /// Use a specific cluster instead of just using the default
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kube_cluster: Option<String>,

    #[serde(default)]
    /// Groups that this host is part of
    pub host_groups: Vec<String>,

    /// CA certificate file for trusting the API
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_cert: Option<PathBuf>,
}

impl KubeHost {
    /// Create a new KubeHost from a hostname
    pub fn from_hostname(hostname: &str) -> Self {
        Self {
            hostname: hostname.to_string(),
            api_port: kube_port_default(),
            kube_cluster: Default::default(),
            host_groups: Default::default(),
            ca_cert: Default::default(),
        }
    }

    /// Set the port for the API
    pub fn with_port(self, api_port: NonZeroU16) -> Self {
        Self { api_port, ..self }
    }

    /// Set the cluster to use from the configuration
    pub fn with_cluster(self, cluster: &str) -> Self {
        Self {
            kube_cluster: Some(cluster.to_string()),
            ..self
        }
    }

    /// Getter for the resulting API URL
    pub fn api_url(&self) -> String {
        format!("https://{}:{}", self.hostname, self.api_port)
    }
}

#[async_trait]
impl GenericHost for KubeHost {
    async fn check_up(&self) -> Result<bool, crate::errors::Error> {
        let client = Client::try_default()
            .await
            .map_err(|_err| Error::ConnectionFailed)?;

        match client.apiserver_version().await {
            Ok(_) => Ok(true),
            Err(err) => Err(Error::Generic(err.to_string())),
        }
    }

    fn try_from_config(config: serde_json::Value) -> Result<Self, Error>
    where
        Self: Sized,
    {
        serde_json::from_value(config).map_err(Error::from)
    }
}

impl TryFrom<&Host> for KubeHost {
    type Error = crate::errors::Error;

    fn try_from(value: &Host) -> Result<Self, Self::Error> {
        let api_port = match value.extra.get("api_port") {
            Some(port) => {
                let port = port.to_owned();
                if let Some(port) = port.as_u64() {
                    NonZeroU16::new(port as u16).ok_or(Error::Configuration(
                        "api_port must be somewhere between 1 and 65535".to_string(),
                    ))?
                } else {
                    return Err(Error::Configuration(
                        "api_port must be a valid number".to_string(),
                    ));
                }
            }
            None => kube_port_default(),
        };

        let hostname = value
            .hostname
            .clone()
            .ok_or(Error::Configuration("hostname is required".to_string()))?;

        Ok(Self {
            hostname,
            api_port,
            kube_cluster: None,
            host_groups: value.host_groups.to_vec(),
            ca_cert: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use crate::host::GenericHost;

    #[test]
    fn test_kube_host_builder() {
        let host = crate::host::kube::KubeHost::from_hostname("localhost");
        assert_eq!(host.hostname, "localhost");
        assert_eq!(
            host.api_port,
            NonZeroU16::new(6443).expect("Failed to parse 6443 as a non-zero u16")
        );
        assert_eq!(host.kube_cluster, None);
        assert_eq!(host.api_url(), "https://localhost:6443");
    }

    #[tokio::test]
    async fn test_kube_check_up() {
        let hostname = match std::env::var("MAREMMA_TEST_KUBE_HOST") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("MAREMMA_TEST_KUBE_HOST not set, skipping test");
                return;
            }
        };

        dbg!(&hostname);

        let host = super::KubeHost::from_hostname(&hostname);
        let result = host.check_up().await;
        assert_eq!(result, Ok(true));
    }

    #[test]
    fn test_kube_host_with_port() {
        let host = crate::host::kube::KubeHost::from_hostname("localhost")
            .with_port(NonZeroU16::new(8443).expect("Failed to parse 8443 as a non-zero u16"));
        assert_eq!(host.hostname, "localhost");
        assert_eq!(
            host.api_port,
            NonZeroU16::new(8443).expect("Failed to parse 8443 as a non-zero u16")
        );
        assert_eq!(host.kube_cluster, None);
        assert_eq!(host.api_url(), "https://localhost:8443");
    }

    #[test]
    fn test_kube_host_with_cluster() {
        let host =
            crate::host::kube::KubeHost::from_hostname("localhost").with_cluster("my-cluster");
        assert_eq!(host.hostname, "localhost");
        assert_eq!(
            host.api_port,
            NonZeroU16::new(6443).expect("Failed to parse 6443 as a non-zero u16")
        );
        assert_eq!(host.kube_cluster, Some("my-cluster".to_string()));
        assert_eq!(host.api_url(), "https://localhost:6443");
    }

    #[test]
    fn test_kube_host_try_from_config() {
        let config = serde_json::json!({
            "hostname": "localhost",
            "api_port": 8443,
            "kube_cluster": "my-cluster",
            "host_groups": ["group1", "group2"]
        });

        let host = crate::host::kube::KubeHost::try_from_config(config).unwrap();
        assert_eq!(host.hostname, "localhost");
        assert_eq!(
            host.api_port,
            NonZeroU16::new(8443).expect("Failed to parse 8443 as a non-zero u16")
        );
        assert_eq!(host.kube_cluster, Some("my-cluster".to_string()));
        assert_eq!(host.host_groups, vec!["group1", "group2"]);
    }
}
