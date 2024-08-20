//! Kubernetes service

use kube::Client;

use crate::prelude::*;

#[derive(Debug, Deserialize)]
/// KubernetesService is a service that checks the availability of a Kubernetes cluster
pub struct KubernetesService {
    /// Name of the service
    pub name: String,
    /// Host to check
    pub host: Host,
    #[serde(deserialize_with = "crate::serde::deserialize_croner_cron")]
    /// Cron schedule for the service
    pub cron_schedule: Cron,
}

#[async_trait]
impl ServiceTrait for KubernetesService {
    async fn run(&self, _host: &entities::host::Model) -> Result<CheckResult, Error> {
        let start_time = chrono::Utc::now();

        let client = match Client::try_default().await {
            Ok(val) => val,
            Err(err) => {
                return Ok(CheckResult {
                    timestamp: start_time,
                    result_text: format!("UNKNOWN: Unable to configure Kubernetes client: {}", err),
                    status: ServiceStatus::Unknown,
                    time_elapsed: chrono::Utc::now() - start_time,
                })
            }
        };

        let (result_text, status) = match client.apiserver_version().await {
            Ok(_) => ("OK".to_string(), ServiceStatus::Ok),
            Err(err) => (format!("CRITICAL: {}", err), ServiceStatus::Critical),
        };

        Ok(CheckResult {
            timestamp: start_time,
            result_text,
            status,
            time_elapsed: chrono::Utc::now() - start_time,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_kubernetes_service() {
        let hostname = match std::env::var("MAREMMA_TEST_KUBE_HOST") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("MAREMMA_TEST_KUBE_HOST not set, skipping test");
                return;
            }
        };

        // TODO: use the kube test host for this test
        let host = Host {
            id: None,
            check: crate::host::HostCheck::None,
            hostname: Some(hostname.clone()),
            host_groups: vec![],
            extra: Default::default(),
        };

        let service = KubernetesService {
            name: "kubernetes".to_string(),
            host,
            cron_schedule: Cron::new("0 0 * * *").parse().unwrap(),
        };

        let result = service
            .run(&entities::host::Model {
                id: Uuid::new_v4(),
                name: "test host".to_string(),
                hostname,
                check: crate::host::HostCheck::None,
            })
            .await
            .unwrap();
        assert!(result.status == ServiceStatus::Ok || result.status == ServiceStatus::Critical);
    }
}
