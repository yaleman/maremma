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
    /// Report active pods that are not in the Ready condition.
    UnreadyPods,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
/// A service that checks a Kubernetes cluster using the configured kube client.
pub struct KubernetesService {
    /// Name of the service.
    pub name: String,
    /// The Kubernetes operation to perform.
    #[serde(default)]
    pub check: KubernetesCheck,
    /// Limit pod checks to this namespace.
    pub namespace: Option<String>,
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
            namespace: self.extract_value(value, "namespace", &self.namespace)?,
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

fn unready_pods(pods: &[Pod]) -> Vec<String> {
    pods.iter()
        .filter_map(|pod| {
            let status = pod.status.as_ref();
            let phase = status.and_then(|status| status.phase.as_deref());
            if phase == Some("Succeeded") {
                return None;
            }

            let namespace = pod.metadata.namespace.as_deref().unwrap_or("default");
            let name = pod.metadata.name.as_deref().unwrap_or("unknown");
            if phase != Some("Running") {
                return Some(format!(
                    "{namespace}/{name} phase={}",
                    phase.unwrap_or("Unknown")
                ));
            }

            let ready = status
                .and_then(|status| status.conditions.as_ref())
                .and_then(|conditions| {
                    conditions
                        .iter()
                        .find(|condition| condition.type_ == "Ready")
                })
                .is_some_and(|condition| condition.status == "True");
            if ready {
                return None;
            }

            let container_statuses = status
                .and_then(|status| status.container_statuses.as_ref())
                .map(Vec::as_slice)
                .unwrap_or_default();
            let unready = container_statuses
                .iter()
                .filter(|container| !container.ready)
                .map(|container| container.name.as_str())
                .collect::<Vec<_>>();
            let restarts = container_statuses
                .iter()
                .map(|container| container.restart_count)
                .sum::<i32>();
            Some(format!(
                "{namespace}/{name} ready=false unready=[{}] restarts={restarts}",
                unready.join(",")
            ))
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
                let pods: Api<Pod> = match self.namespace.as_deref() {
                    Some(namespace) => Api::namespaced(client, namespace),
                    None => Api::all(client),
                };
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
            KubernetesCheck::UnreadyPods => {
                let pods: Api<Pod> = match self.namespace.as_deref() {
                    Some(namespace) => Api::namespaced(client, namespace),
                    None => Api::all(client),
                };
                match pods.list(&Default::default()).await {
                    Ok(pods) => {
                        let unready = unready_pods(&pods.items);
                        if unready.is_empty() {
                            let restart_count = pods
                                .items
                                .iter()
                                .filter_map(|pod| pod.status.as_ref())
                                .filter_map(|status| status.container_statuses.as_ref())
                                .flatten()
                                .map(|container| container.restart_count)
                                .sum::<i32>();
                            (
                                format!(
                                    "OK: {} pods are ready or completed; restarts={restart_count}",
                                    pods.items.len()
                                ),
                                ServiceStatus::Ok,
                            )
                        } else {
                            (
                                format!(
                                    "CRITICAL: {} pods are not ready: {}",
                                    unready.len(),
                                    unready.join(" ")
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
    async fn run(
        &self,
        host: &entities::host::Model,
        _context: &CheckExecutionContext,
    ) -> Result<CheckResult, MaremmaError> {
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
    use k8s_openapi::api::core::v1::{ContainerStatus, PodCondition};
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

    fn ready_pod(name: &str, phase: &str, ready: bool, restarts: i32) -> Pod {
        let mut pod = pod(name, "clickstack", Some(phase));
        let status = pod.status.as_mut().expect("pod status should exist");
        status.conditions = Some(vec![PodCondition {
            last_probe_time: None,
            last_transition_time: None,
            message: None,
            observed_generation: None,
            reason: None,
            status: if ready { "True" } else { "False" }.to_string(),
            type_: "Ready".to_string(),
        }]);
        status.container_statuses = Some(vec![ContainerStatus {
            allocated_resources: None,
            allocated_resources_status: None,
            container_id: None,
            image: "example.invalid/image".to_string(),
            image_id: "example.invalid/image@sha256:test".to_string(),
            last_state: None,
            name: "app".to_string(),
            ready,
            resources: None,
            restart_count: restarts,
            started: Some(true),
            state: None,
            stop_signal: None,
            user: None,
            volume_mounts: None,
        }]);
        pod
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
    fn filters_unready_pods_and_ignores_old_restarts() {
        let pods = vec![
            ready_pod("ready", "Running", true, 3),
            ready_pod("unready", "Running", false, 2),
            ready_pod("completed", "Succeeded", false, 0),
            pod("pending", "clickstack", Some("Pending")),
            pod("failed", "clickstack", Some("Failed")),
        ];

        assert_eq!(
            unready_pods(&pods),
            vec![
                "clickstack/unready ready=false unready=[app] restarts=2",
                "clickstack/pending phase=Pending",
                "clickstack/failed phase=Failed",
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
            "namespace": "clickstack",
            "cron_schedule": "*/10 * * * *"
        });

        let service = Service::try_from(&value).expect("Failed to parse Kubernetes service");
        assert_eq!(service.service_type, ServiceType::Kubernetes);
    }
}
