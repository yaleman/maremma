//! Kubernetes service

use kube::Client;

use super::prelude::*;
use crate::prelude::*;

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
/// KubernetesService is a service that checks the availability of a Kubernetes cluster
pub struct KubernetesService {
    /// Name of the service
    pub name: String,
    /// Host to check
    pub host: Host,
    #[serde(with = "crate::serde::cron")]
    /// The cron schedule for this service
    #[schemars(with = "String")]
    pub cron_schedule: Cron,
    /// Add random jitter in 0..n seconds to the check
    pub jitter: Option<u16>,
}

impl ConfigOverlay for KubernetesService {
    fn overlay_host_config(&self, value: &Map<String, Json>) -> Result<Box<Self>, Error> {
        let name = self.extract_string(value, "name", &self.name);
        let cron_schedule = self.extract_cron(value, "cron_schedule", &self.cron_schedule)?;

        Ok(Box::new(Self {
            name,
            cron_schedule,
            host: self.extract_value(value, "host", &self.host)?,
            jitter: self.extract_value(value, "jitter", &self.jitter)?,
        }))
    }
}

#[async_trait]
impl ServiceTrait for KubernetesService {
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        let start_time: DateTime<Utc> = chrono::Utc::now();

        let _config = self.overlay_host_config(&self.get_host_config(&self.name, host)?)?;

        let client = match Client::try_default().await {
            Ok(val) => val,
            Err(err) => {
                return Ok(CheckResult {
                    timestamp: start_time,
                    result_text: format!("UNKNOWN: Unable to configure Kubernetes client: {err}"),
                    status: ServiceStatus::Unknown,
                    time_elapsed: chrono::Utc::now() - start_time,
                })
            }
        };

        let (result_text, status) = match client.apiserver_version().await {
            Ok(_) => ("OK".to_string(), ServiceStatus::Ok),
            Err(err) => (format!("CRITICAL: {err}"), ServiceStatus::Critical),
        };

        Ok(CheckResult {
            timestamp: start_time,
            result_text,
            status,
            time_elapsed: chrono::Utc::now() - start_time,
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

#[cfg(test)]
mod tests {
    use entities::host::test_host;

    use crate::db::tests::test_setup;
    use crate::host::kube::KubeHost;

    use super::*;

    #[tokio::test]
    async fn test_kubernetes_service() {
        let _ = test_setup().await.expect("Failed to set up test env");

        let hostname = match std::env::var("MAREMMA_TEST_KUBE_HOST") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("MAREMMA_TEST_KUBE_HOST not set, skipping test");
                return;
            }
        };

        let host = Host {
            check: crate::host::HostCheck::Kubernetes,
            hostname: Some(hostname.clone()),
            ..test_host().into()
        };
        let kubehost = KubeHost::try_from(&host).expect("Failed to convert host to kubehost");
        kubehost
            .check_up()
            .await
            .expect("Failed to check_up kubehost");

        let service = KubernetesService {
            name: "kubernetes".to_string(),
            host,
            cron_schedule: std::str::FromStr::from_str("0 0 * * *")
                .expect("Failed to parse cron"),
            jitter: None,
        };

        let result = service
            .run(&entities::host::Model {
                id: Uuid::new_v4(),
                name: "test host".to_string(),
                hostname,
                check: crate::host::HostCheck::None,
                config: json!({}),
            })
            .await
            .expect("Failed to run");
        assert!(result.status == ServiceStatus::Ok || result.status == ServiceStatus::Critical);
    }
}
