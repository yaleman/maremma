use std::collections::HashMap;
use std::sync::Arc;

use crate::host::fakehost::FakeHost;
use crate::prelude::*;

pub type ServiceTable = HashMap<String, Service>;

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Configuration {
    pub hosts: Vec<Host>,

    #[serde(default)]
    pub local_services: FakeHost,

    // This is something we need to deserialize later because it'll end up being a mess
    #[serde(skip_serializing)]
    pub services: Option<serde_json::Value>,

    #[serde(skip_serializing, skip_deserializing)]
    pub service_table: ServiceTable,

    /// A hashmap where the key is the host group and it contains a list of services that apply.
    #[serde(skip_serializing, skip_deserializing)]
    pub host_group_services: HashMap<String, Vec<String>>,

    /// A hashmap of host_group and host members (ids)
    #[serde(skip_serializing, skip_deserializing)]
    pub host_group_members: HashMap<String, Vec<Arc<String>>>,
}

impl Configuration {
    pub fn new(filename: &str) -> Result<Self, Error> {
        Self::new_from_string(&std::fs::read_to_string(filename)?)
    }

    pub fn new_from_string(config: &str) -> Result<Self, Error> {
        let mut res: Configuration = serde_json::from_str(config)?;

        res.parse_service_list()?;
        res.build_host_group_services();
        res.build_host_group_members();

        Ok(res)
    }

    pub fn parse_service_list(&mut self) -> Result<(), Error> {
        let services = match &self.services {
            None => return Ok(()),
            Some(val) => val,
        };

        let services_array = match services.as_array() {
            None => return Err(Error::Generic("Services must be an array".to_string())),
            Some(arr) => arr,
        };

        for service_value in services_array {
            let service = Service::try_from(service_value)?;
            self.service_table.insert(service.id(), service);
        }

        Ok(())
    }

    pub fn build_host_group_services(&mut self) {
        for (service_id, service) in self.service_table.iter() {
            for host_group in service.host_groups.iter() {
                if !self.host_group_services.contains_key(host_group) {
                    self.host_group_services
                        .insert(host_group.to_string(), vec![]);
                }
                self.host_group_services
                    .get_mut(host_group)
                    .unwrap()
                    .push(service_id.clone());
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
                self.host_group_members
                    .get_mut(host_group)
                    .unwrap()
                    .push(host.host_id());
            }
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_config_new() {
        let config = r#"{
            "hosts": [
                {
                    "name" : "foo.bar"
                }
            ]
        }"#;
        let config = crate::config::Configuration::new_from_string(config).unwrap();
        assert_eq!(config.hosts.len(), 1);
    }
}
