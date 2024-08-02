use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::host::fakehost::FakeHost;
use crate::host::{Host, HostCheck};
use crate::prelude::*;
use crate::services::check::{generate_service_check_id, ServiceCheck};
// use crate::services::kubernetes::KubernetesService;

pub type ServiceTable = HashMap<String, Service>;

fn default_database_file() -> String {
    "maremma.sqlite".to_string()
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Configuration {
    #[serde(default = "default_database_file")]
    pub database_file: String,

    pub hosts: Vec<Host>,

    #[serde(default)]
    pub local_services: FakeHost,

    // This is something we need to deserialize later because it's messy
    #[serde(skip_serializing)]
    pub services: Option<serde_json::Value>,

    #[serde(skip)]
    /// List of services by service id
    pub service_table: ServiceTable,

    /// A hashmap where the key is the host group and it contains a list of service_ids that apply.
    #[serde(skip)]
    pub host_group_services: HashMap<String, Vec<String>>,

    /// A hashmap of host_group and host members (ids)
    #[serde(skip)]
    pub host_group_members: HashMap<String, Vec<Arc<String>>>,

    /// A hashmap of service checks, keyed by the service check id
    #[serde(skip)]
    pub service_checks: ServiceChecks,
}

impl Configuration {
    pub async fn new(filename: Option<PathBuf>) -> Result<Self, Error> {
        let filename = filename.unwrap_or(PathBuf::from(DEFAULT_CONFIG_FILE));
        if !filename.exists() {
            return Err(Error::ConfigFileNotFound(
                filename.to_string_lossy().to_string(),
            ));
        }
        debug!("Loading config from {:?}", filename);
        Self::new_from_string(&tokio::fs::read_to_string(filename).await?).await
    }

    pub async fn new_from_string(config: &str) -> Result<Self, Error> {
        let mut res: Configuration = serde_json::from_str(config)?;

        if !res.local_services.services.is_empty() {
            res.hosts
                .push(Host::new(LOCAL_SERVICE_HOST_NAME, HostCheck::None));
        }

        res.update_service_table()?;
        res.build_host_group_services().await;
        res.build_host_group_members();

        res.update_service_checks().await;
        Ok(res)
    }

    pub fn update_service_table(&mut self) -> Result<(), Error> {
        let services = match &self.services {
            None => {
                debug!("No services!");
                return Ok(());
            }
            Some(val) => val,
        };

        let services_array = match services.as_array() {
            None => return Err(Error::Generic("Services must be an array".to_string())),
            Some(arr) => arr,
        };

        for service_value in services_array {
            let service = match Service::try_from(service_value) {
                Ok(service) => service,
                Err(err) => {
                    error!("Failed to parse service: {:?}", err);
                    continue;
                }
            };
            debug!("Service: {:?}", service);
            self.service_table.insert(service.id(), service);
        }

        Ok(())
    }

    pub fn get_service_by_name(&self, name: &str) -> Option<&Service> {
        trace!("looking for service {}", name);
        self.service_table.values().find(|service| {
            trace!("Comparing {} to needle {}", service.name, name);
            service.name == name
        })
    }

    pub async fn build_host_group_services(&mut self) {
        for (service_id, service) in self.service_table.iter() {
            for host_group_name in service.host_groups.iter() {
                if !self.host_group_services.contains_key(host_group_name) {
                    self.host_group_services
                        .insert(host_group_name.to_string(), vec![]);
                }

                if let Some(host_group_list) = self.host_group_services.get_mut(host_group_name) {
                    host_group_list.push(service_id.clone());
                } else {
                    error!(
                        "Couldn't find host group which we just added!: {}",
                        host_group_name
                    );
                }
            }
        }

        debug!("{:?}", self.service_table);

        // has to be done after the above because service_table isn't populated until after that
        for service_name in self.local_services.services.iter() {
            if let Some(service) = self.get_service_by_name(service_name) {
                // self.service_table.insert(service.clone(), service);
                let host_id = Host::generate_host_id(LOCAL_SERVICE_HOST_NAME, &HostCheck::None);
                self.service_checks.write().await.insert(
                    generate_service_check_id(&host_id, &service.id()),
                    ServiceCheck::new(Arc::new(host_id), service.id()),
                );
            } else {
                error!("Couldn't find service '{}' in config!", service_name);
                continue;
            }
        }
    }

    pub fn build_host_group_members(&mut self) {
        for host in self.hosts.iter() {
            for host_group in host.host_groups.iter() {
                if !self.host_group_members.contains_key(host_group) {
                    self.host_group_members
                        .insert(host_group.to_string(), vec![]);
                }
                if let Some(host_group) = self.host_group_members.get_mut(host_group) {
                    host_group.push(host.host_id());
                } else {
                    error!("We just added this host group!")
                }
            }
        }
    }

    pub async fn update_service_checks(&mut self) {
        // TODO: finish host checks
        // for host in self.hosts.iter() {
        //     let _host_check_service = match host.check {
        //         HostCheck::None => continue,
        //         HostCheck::Ping => continue,
        //         HostCheck::SshHost => todo!(),
        //         HostCheck::KubeHost => KubernetesService {
        //             name: host.name.to_owned(),
        //             host: host.clone(),
        //             cron_schedule: "* * * * * *".parse().unwrap(),
        //         },
        //     };
        // }

        for (host_group_id, service_ids) in &self.host_group_services {
            for service_id in service_ids {
                for host_id in self
                    .host_group_members
                    .get(host_group_id)
                    .cloned()
                    .unwrap_or(vec![])
                {
                    let service_check_id = generate_service_check_id(&host_id, service_id);
                    // check if the servicecheck exists already

                    if let std::collections::hash_map::Entry::Vacant(e) = self
                        .service_checks
                        .write()
                        .await
                        .entry(service_check_id.clone())
                    {
                        debug!(
                            "Adding service check: {} to host: {}",
                            service_check_id, host_id
                        );
                        // create a new service check
                        e.insert(ServiceCheck::new(host_id.clone(), service_id.clone()));
                    } else {
                        // TODO: update the service check
                        debug!("Service check: {} already exists", service_check_id);
                    }
                }
            }
        }
    }

    /// Get the next service check to run
    pub async fn get_next_service_check(&self) -> Option<String> {
        // Try and get an urgent one first
        if let Some(id) = self
            .service_checks
            .write()
            .await
            .iter_mut()
            .find_map(|(id, check)| {
                if let ServiceStatus::Urgent = check.status {
                    check.checkout();
                    return Some(id.to_owned());
                }
                None
            })
        {
            return Some(id);
        }
        let now = Some(chrono::Utc::now());

        self.service_checks
            .write()
            .await
            .iter_mut()
            .find_map(|(id, check)| {
                if let ServiceStatus::Checking = check.status {
                    // we're already checking this
                    return None;
                }

                if check.is_due(self, now).unwrap_or(false) {
                    debug!("Returning {}", check.check_id());
                    check.checkout();
                    Some(id.to_owned())
                } else {
                    trace!("No check found");
                    None
                }
            })
    }

    pub async fn run_check(&self, next_check_id: &str) -> Result<(String, ServiceStatus), Error> {
        let check = self.service_checks.read().await;
        let check = check
            .get(next_check_id)
            .ok_or(Error::ServiceCheckNotFound(next_check_id.to_string()))?;

        let host = self
            .hosts
            .iter()
            .find(|host| host.host_id() == check.host_id)
            .ok_or(Error::HostNotFound((*check.host_id).clone()))?;

        let service = self
            .service_table
            .get(&check.service_id)
            .ok_or(Error::ServiceNotFound)?;
        if let Some(config) = &service.config {
            match config.run(host).await {
                Ok(val) => Ok((host.hostname(), val)),
                Err(err) => Err(err),
            }
        } else {
            Err(Error::ServiceConfigNotFound(next_check_id.to_string()))
        }
    }

    /// find the next time we need to wake up
    pub async fn find_next_wakeup(&self) -> DateTime<Utc> {
        let mut next_wakeup: Option<DateTime<Utc>> = None;

        for (_id, check) in self.service_checks.read().await.iter() {
            if let Ok(cron) = check.get_cron(self) {
                if let Ok(next_runtime) = cron.find_next_occurrence(&check.last_check, false) {
                    match next_wakeup {
                        Some(wakeup) => {
                            if next_runtime < wakeup {
                                next_wakeup = Some(next_runtime);
                            }
                        }
                        None => {
                            next_wakeup = Some(next_runtime);
                        }
                    }
                }
            }
        }
        next_wakeup.unwrap_or(chrono::Utc::now() + TimeDelta::seconds(1))
    }

    pub fn get_host(&self, host_id: &str) -> Option<&Host> {
        self.hosts.iter().find(|host| *host.host_id() == host_id)
    }

    pub fn get_service(&self, service_id: &str) -> Option<&Service> {
        self.service_table.get(service_id)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::config::Configuration;
    use crate::host::{Host, HostCheck};
    use crate::services::generate_service_id;

    #[tokio::test]
    async fn test_config_new() {
        let config = r#"{
            "hosts": [
                {
                    "name" : "foo.bar"
                }
            ]
        }"#;
        let config = Configuration::new_from_string(config).await.unwrap();
        assert_eq!(config.hosts.len(), 1);
    }

    #[tokio::test]
    async fn test_example_config() {
        #[allow(clippy::expect_used)]
        let config = Configuration::new(Some(PathBuf::from("maremma.example.json")))
            .await
            .expect("Failed to load example config");

        assert!(config.get_next_service_check().await.is_some());

        let expected_host = Host::generate_host_id(&"example.com", &HostCheck::default());

        assert!(config.get_host(&expected_host).is_some());

        let service_id = generate_service_id("check_ntp_time", &crate::services::ServiceType::Ssh);

        // check we're parsing services
        assert!(config.get_service(&service_id).is_some());

        // the example config should have a service check pending on startup
        assert!(config.find_next_wakeup().await <= chrono::Utc::now());
    }
}
