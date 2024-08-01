use kube::Client;

use crate::prelude::*;

pub(crate) fn kube_port_default() -> u16 {
    6443
}

#[derive(Default, Deserialize, Serialize, Debug)]
pub struct KubeHost {
    pub api_hostname: String,
    /// Defaults to 6443
    #[serde(default = "kube_port_default")]
    pub api_port: u16,
    /// Use a specific cluster instead of just using the default
    pub kube_cluster: Option<String>,

    #[serde(default)]
    pub host_groups: Vec<String>,
}

impl KubeHost {
    pub fn from_hostname(hostname: &str) -> Self {
        Self {
            api_hostname: hostname.to_string(),
            api_port: kube_port_default(),
            ..Default::default()
        }
    }
    pub fn with_port(self, api_port: u16) -> Self {
        Self { api_port, ..self }
    }
    pub fn with_cluster(self, cluster: &str) -> Self {
        Self {
            kube_cluster: Some(cluster.to_string()),
            ..self
        }
    }
    pub fn api_url(&self) -> String {
        format!("https://{}:{}", self.api_hostname, self.api_port)
    }
}

#[async_trait]
impl GenericHost for KubeHost {
    fn id(&self) -> String {
        sha256::digest(&format!(
            "{}:{}",
            self.api_url(),
            self.kube_cluster.as_ref().unwrap_or(&"default".to_string())
        ))
    }
    fn name(&self) -> String {
        format!("KubeHost({})", self.api_url())
    }
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

#[cfg(test)]
mod tests {
    #[test]
    fn test_kube_host_builderd() {
        let host = crate::host::kube::KubeHost::from_hostname("localhost");
        assert_eq!(host.api_hostname, "localhost");
        assert_eq!(host.api_port, 6443);
        assert_eq!(host.kube_cluster, None);
        assert_eq!(host.api_url(), "https://localhost:6443");
    }

    #[tokio::test]
    async fn test_kube_check_up() {
        use super::*;

        let hostname = match std::env::var("MAREMMA_TEST_KUBE_HOST") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("MAREMMA_TEST_KUBE_HOST not set, skipping test");
                return;
            }
        };

        eprintln!("Testing kube host: {}", hostname);

        let host = crate::host::kube::KubeHost::from_hostname(&hostname);
        let result = host.check_up().await;
        assert_eq!(result, Ok(true));
    }
}
