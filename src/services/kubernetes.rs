//! Kubernetes service checks.

use k8s_openapi::api::core::v1::Pod;
use kube::{Api, Client};
use schemars::JsonSchema;

use super::prelude::*;
use crate::prelude::*;

#[derive(Debug, Default, Deserialize, JsonSchema, Serialize, Copy, Clone, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
/// The Kubernetes operation performed by a service check.
pub enum KubernetesCheck {
    /// Confirm that the API server responds to a version request.
    #[default]
    ApiAvailable,
    /// Report pods that are neither running nor completed successfully.
    UnhealthyPods,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
/// A service that checks a Kubernetes cluster using the configured kube client.
pub struct KubernetesService {
    /// Name of the service.
    pub name: String,
    /// The Kubernetes operation to perform.
    #[serde(default)]
    pub check: KubernetesCheck,
    #[serde(with = "crate::serde::cron")]
    #[schemars(with = "String")]
    /// Schedule for the service.
    pub cron_schedule: Cron,
    /// Add random jitter in 0..n seconds to the check.
    pub jitter: Option<u16>,
}

impl ConfigOverlay for KubernetesService {
    fn overlay_host_config(&self, value: &Map<String, Json>) -> Result<Box<Self>, MaremmaError> {
        Ok(Box::new(Self {
            name: self.extract_string(value, "name", &self.name),
            check: self.extract_value(value, "check", &self.check)?,
            cron_schedule: self.extract_cron(value, "cron_schedule", &self.cron_schedule)?,
            jitter: self.extract_value(value, "jitter", &self.jitter)?,
        }))
    }
}

fn unhealthy_pods(pods: &[Pod]) -> Vec<String> {
    pods.iter()
        .filter_map(|pod| {
            let phase = pod
                .status
                .as_ref()
                .and_then(|status| status.phase.as_deref());
            if matches!(phase, Some("Running" | "Succeeded")) {
                return None;
            }

            let namespace = pod.metadata.namespace.as_deref().unwrap_or("default");
            let name = pod.metadata.name.as_deref().unwrap_or("unknown");
            Some(format!("{namespace}/{name}={}", phase.unwrap_or("Unknown")))
        })
        .collect()
}

impl KubernetesService {
    async fn run_check(&self, client: Client) -> (String, ServiceStatus) {
        match self.check {
            KubernetesCheck::ApiAvailable => match client.apiserver_version().await {
                Ok(version) => (
                    format!("OK: Kubernetes {}", version.git_version),
                    ServiceStatus::Ok,
                ),
                Err(err) => (format!("CRITICAL: {err}"), ServiceStatus::Critical),
            },
            KubernetesCheck::UnhealthyPods => {
                let pods: Api<Pod> = Api::all(client);
                match pods.list(&Default::default()).await {
                    Ok(pods) => {
                        let unhealthy = unhealthy_pods(&pods.items);
                        if unhealthy.is_empty() {
                            (
                                "OK: All pods are running or succeeded".to_string(),
                                ServiceStatus::Ok,
                            )
                        } else {
                            (
                                format!(
                                    "CRITICAL: {} pods are not running or succeeded {}",
                                    unhealthy.len(),
                                    unhealthy.join(" ")
                                ),
                                ServiceStatus::Critical,
                            )
                        }
                    }
                    Err(err) => (format!("CRITICAL: {err}"), ServiceStatus::Critical),
                }
            }
        }
    }
}

#[async_trait]
impl ServiceTrait for KubernetesService {
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, MaremmaError> {
        let start_time = Utc::now();
        let config = self.overlay_host_config(&self.get_host_config(&self.name, host)?)?;

        let client = match Client::try_default().await {
            Ok(client) => client,
            Err(err) => {
                return Ok(CheckResult {
                    timestamp: start_time,
                    result_text: format!("UNKNOWN: Unable to configure Kubernetes client: {err}"),
                    status: ServiceStatus::Unknown,
                    time_elapsed: Utc::now() - start_time,
                });
            }
        };

        let (result_text, status) = config.run_check(client).await;
        Ok(CheckResult {
            timestamp: start_time,
            result_text,
            status,
            time_elapsed: Utc::now() - start_time,
        })
    }

    fn as_json_pretty(&self, host: &entities::host::Model) -> Result<String, MaremmaError> {
        let config = self.overlay_host_config(&self.get_host_config(&self.name, host)?)?;
        Ok(serde_json::to_string_pretty(&config)?)
    }

    fn jitter_value(&self) -> u32 {
        self.jitter.unwrap_or(0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn pod(name: &str, namespace: &str, phase: Option<&str>) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                ..Default::default()
            },
            status: Some(k8s_openapi::api::core::v1::PodStatus {
                phase: phase.map(str::to_string),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn filters_running_and_succeeded_pods() {
        let pods = vec![
            pod("running", "default", Some("Running")),
            pod("job", "jobs", Some("Succeeded")),
            pod("broken", "default", Some("CrashLoopBackOff")),
            pod("pending", "apps", Some("Pending")),
            pod("unknown", "apps", None),
        ];

        assert_eq!(
            unhealthy_pods(&pods),
            vec![
                "default/broken=CrashLoopBackOff",
                "apps/pending=Pending",
                "apps/unknown=Unknown",
            ]
        );
    }

    #[test]
    fn parses_public_service_configuration() {
        let value = json!({
            "name": "pods",
            "service_type": "kubernetes",
            "host_groups": ["k8s_leader"],
            "check": "unhealthy_pods",
            "cron_schedule": "*/10 * * * *"
        });

        let service = Service::try_from(&value).expect("Failed to parse Kubernetes service");
        assert_eq!(service.service_type, ServiceType::Kubernetes);
    }
}
