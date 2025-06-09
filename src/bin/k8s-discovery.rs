use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::num::NonZeroU16;

use k8s_openapi::api::core::v1::{Namespace, Pod, Service};
use k8s_openapi::api::networking::v1::Ingress;
use kube::api::ListParams;
use kube::{Api, Client};
use maremma::errors::Error;
use maremma::log::setup_logging;
use serde::{Deserialize, Serialize};
use tracing::*;

pub const MAREMMA_SERVICE_NAME: &str = "maremma.terminaloutcomes.com";

#[cfg(not(tarpaulin_include))]
#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
enum Protocol {
    Tcp,
    Udp,
    Http,
    #[default]
    Unknown,
}

#[cfg(not(tarpaulin_include))]
#[derive(Debug, Serialize, Deserialize)]
struct K8sPod {
    name: String,
    namespace: String,
    annotations: HashMap<String, String>,
}

#[cfg(not(tarpaulin_include))]
#[derive(Debug, Serialize, Deserialize)]
struct K8sIngress {
    name: String,
    namespace: String,
    annotations: HashMap<String, String>,
}

#[cfg(not(tarpaulin_include))]
#[derive(Debug, Deserialize, Serialize)]
struct K8sService {
    name: String,
    namespace: String,
    cluster_ips: Vec<IpAddr>,
    external_ips: Vec<IpAddr>,
    ports: Vec<NonZeroU16>,
    protocol: Protocol,
    annotations: HashMap<String, String>,
}

#[cfg(not(tarpaulin_include))]
/// Check if the maremma annotations are in a set of annotations
fn has_maremma_annotations(input: Option<&BTreeMap<String, String>>) -> bool {
    if let Some(annotations) = input {
        for (key, _value) in annotations.iter() {
            if key.starts_with(MAREMMA_SERVICE_NAME) {
                return true;
            }
        }
    }
    false
}

#[cfg(not(tarpaulin_include))]
impl TryFrom<(&Namespace, &Pod)> for K8sPod {
    type Error = Error;

    fn try_from(value: (&Namespace, &Pod)) -> Result<Self, Self::Error> {
        let (namespace, pod) = value;
        Ok(K8sPod {
            namespace: namespace
                .metadata
                .name
                .clone()
                .ok_or(Error::InvalidInput("No namespace?".to_string()))?,
            name: pod
                .metadata
                .name
                .clone()
                .ok_or(Error::InvalidInput("No pod name?".to_string()))?,
            annotations: pod
                .metadata
                .annotations
                .as_ref()
                .iter()
                .flat_map(|annotations| {
                    annotations.iter().filter_map(|(key, value)| {
                        if key.starts_with(MAREMMA_SERVICE_NAME) {
                            Some((
                                key.replace(&format!("{}/", MAREMMA_SERVICE_NAME), ""),
                                value.clone(),
                            ))
                        } else {
                            None
                        }
                    })
                })
                .collect(),
        })
    }
}

#[cfg(not(tarpaulin_include))]
impl TryFrom<(&Namespace, &Ingress)> for K8sIngress {
    type Error = Error;

    fn try_from(value: (&Namespace, &Ingress)) -> Result<Self, Self::Error> {
        let (namespace, ingress) = value;

        let mut res = Self {
            name: ingress
                .metadata
                .name
                .clone()
                .ok_or(Error::InvalidInput("No ingress name?".to_string()))?,
            namespace: namespace
                .metadata
                .name
                .clone()
                .ok_or(Error::InvalidInput("No namespace?".to_string()))?,
            annotations: HashMap::new(),
        };

        if let Some(annotations) = ingress.metadata.annotations.as_ref() {
            for (key, value) in annotations.iter() {
                if key.starts_with(MAREMMA_SERVICE_NAME) {
                    debug!("ingress annotation: {}={}", key, value);
                    res.annotations.insert(
                        key.replace(&format!("{}/", MAREMMA_SERVICE_NAME), ""),
                        value.clone(),
                    );
                }
            }
        }

        Ok(res)
    }
}

#[cfg(not(tarpaulin_include))]
impl TryFrom<(&Namespace, &Service)> for K8sService {
    type Error = Error;
    fn try_from(input: (&Namespace, &Service)) -> Result<Self, Error> {
        let (namespace, service) = input;
        // debug!("service: {:?}", service);
        let mut res = Self {
            namespace: namespace
                .metadata
                .name
                .clone()
                .ok_or(Error::Generic("No namespace?".to_string()))?,
            name: service
                .metadata
                .name
                .clone()
                .ok_or(Error::Generic("No service name?".to_string()))?,
            cluster_ips: vec![],
            external_ips: vec![],
            ports: vec![],
            protocol: Protocol::default(),
            annotations: HashMap::new(),
        };

        if let Some(annotations) = service.metadata.annotations.as_ref() {
            for (key, value) in annotations.iter() {
                if key.starts_with(MAREMMA_SERVICE_NAME) {
                    debug!("service annotation: {}={}", key, value);
                    res.annotations.insert(
                        key.replace(&format!("{}/", MAREMMA_SERVICE_NAME), ""),
                        value.clone(),
                    );
                }
            }
        }

        if let Some(spec) = &service.spec {
            if let Some(cluster_ip) = &spec.cluster_ip {
                if !cluster_ip.is_empty() && cluster_ip != "None" {
                    res.cluster_ips.push(cluster_ip.parse().map_err(|err| {
                        error!(
                            "cluster IP {} on service {} bad: {:?}",
                            cluster_ip, res.name, err
                        );
                        Error::from(err)
                    })?);
                }
            }
            if let Some(cluster_ips) = &spec.cluster_ips {
                for ip in cluster_ips {
                    if ip.is_empty() || ip == "None" {
                        continue;
                    }
                    res.cluster_ips.push(ip.parse().map_err(|err| {
                        error!("cluster IPs {} on service {} bad: {:?}", ip, res.name, err);
                        Error::from(err)
                    })?);
                }
            }
            if let Some(external_ips) = &spec.external_ips {
                for ip in external_ips {
                    if ip.is_empty() || ip == "None" {
                        error!("Empty external IP on service {}", res.name);
                        continue;
                    }
                    debug!("trying {}", ip);
                    res.external_ips.push(ip.parse().map_err(|err| {
                        error!("external IP {} on service {} bad: {:?}", ip, res.name, err);
                        Error::from(err)
                    })?);
                }
            }

            if let Some(ports) = &spec.ports {
                for port in ports {
                    if let Some(port) = NonZeroU16::new(port.port as u16) {
                        res.ports.push(port);
                    } else {
                        error!("Invalid port: {} on service {}", port.port, res.name);
                    }
                }
            }
        }
        Ok(res)
    }
}

#[cfg(not(tarpaulin_include))]
#[derive(Debug, Deserialize, Serialize, Default)]
struct K8sServiceDiscovery {
    services: Vec<K8sService>,
    ingress: Vec<K8sIngress>,
    pods: Vec<K8sPod>,
}

#[cfg(not(tarpaulin_include))]
async fn discover_ingress(
    client: Client,
    namespace: &Namespace,
    namespace_name: &str,
) -> Result<Vec<K8sIngress>, Error> {
    let ingress_api: Api<Ingress> = Api::namespaced(client.clone(), namespace_name);

    let ingresses = ingress_api.list(&ListParams::default()).await?;
    Ok(ingresses
        .iter()
        .flat_map(|ingress| {
            if has_maremma_annotations(ingress.metadata.annotations.as_ref()) {
                K8sIngress::try_from((namespace, ingress)).ok()
            } else {
                debug!("  skipping ingress: {:?}", ingress.metadata.name);
                None
            }
        })
        .collect::<Vec<K8sIngress>>())
}

#[tokio::main]
#[cfg(not(tarpaulin_include))] // ignore for code coverage
async fn main() -> Result<(), Error> {
    use kube::config::KubeConfigOptions;

    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let ignore_annotation = format!("{}/{}", MAREMMA_SERVICE_NAME, "ignore");

    if let Err(err) = setup_logging(true, true, false) {
        eprintln!("Error setting up logging: {:?}", err);
        return Err(Error::Generic("Error setting up logging".to_string()));
    };

    debug!("Discovering namespaces");
    let mut discovery_data = K8sServiceDiscovery::default();

    let client = Client::try_default().await.map_err(Error::from)?;

    let namespaces: Api<Namespace> = Api::all(client.clone());
    let res = namespaces.list(&ListParams::default()).await?;
    for namespace in res {
        info!("namespace: {:?}", namespace.metadata.name);

        if let Some(annotations) = namespace.metadata.annotations.as_ref() {
            if let Some(ignore_key) = annotations.get(&ignore_annotation) {
                if ignore_key == "true" {
                    info!("  ignoring namespace {:?}", &namespace.metadata.name);
                    continue;
                } else {
                    warn!("Got ignore key but not true: {}", ignore_key);
                }
            }
        }

        if let Some(namespace_name) = &namespace.metadata.name {
            // get the pods
            let pods: Api<Pod> = Api::namespaced(client.clone(), namespace_name);
            let res = pods.list(&ListParams::default()).await?;

            for pod in res {
                if has_maremma_annotations(pod.metadata.annotations.as_ref()) {
                    // info!("pod: {:?}", &pod);
                    discovery_data
                        .pods
                        .push(K8sPod::try_from((&namespace.clone(), &pod))?);
                }
            }
            // get the services

            let service_api: Api<Service> = Api::namespaced(client.clone(), namespace_name);
            let services = service_api.list(&ListParams::default()).await?;
            for service in services {
                if has_maremma_annotations(service.metadata.annotations.as_ref()) {
                    // info!("service: {:?}", &service);
                    discovery_data
                        .services
                        .push(K8sService::try_from((&namespace.clone(), &service))?);
                    if let Some(annotations) = service.metadata.annotations {
                        for (key, value) in annotations.iter() {
                            if key.starts_with(MAREMMA_SERVICE_NAME) {
                                info!("  annotation: {}={}", key, value);
                            }
                        }
                    }
                }
            }
            // https://docs.rs/kube-client/0.93.1/kube_client/config/struct.Config.html
            let config = kube::Config::from_kubeconfig(&KubeConfigOptions {
                context: None,
                cluster: None,
                user: None,
            })
            .await?;

            let ingress_client = kube::Client::try_from(config)?;

            // get the ingressen
            discovery_data
                .ingress
                .extend(discover_ingress(ingress_client, &namespace, namespace_name).await?);
        } else {
            error!("namespace has no name?");
        }

        debug!("#################################################################");
    }

    info!("Discovery data: {:#?}", discovery_data);
    info!("Found {} services", discovery_data.services.len());
    info!("Found {} ingresses", discovery_data.ingress.len());
    info!("Found {} pods", discovery_data.pods.len());

    Ok(())
}
