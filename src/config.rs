use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::host::fakehost::FakeHost;
use crate::host::{Host, HostCheck};
use crate::prelude::*;

fn default_database_file() -> String {
    "maremma.sqlite".to_string()
}

fn default_listen_address() -> String {
    "127.0.0.1".to_string()
}

fn default_max_concurrent_checks() -> usize {
    let cpus = num_cpus::get();
    debug!("Detected {} CPUs", cpus);
    std::cmp::max(cpus.saturating_sub(2), 1)
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
}
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Configuration {
    #[serde(default = "default_database_file")]
    pub database_file: String,

    #[serde(default = "default_listen_address")]
    pub listen_address: String,

    //// Defaults to 8888
    pub listen_port: Option<u16>,

    pub hosts: HashMap<String, Host>,

    #[serde(default)]
    pub local_services: FakeHost,

    // This is something we need to deserialize later because it's messy
    #[serde(skip_serializing)]
    pub services: Option<serde_json::Value>,

    /// The frontend URL ie `https://maremma.example.com` used for things like OIDC
    pub frontend_url: Option<String>,

    #[serde(default)]
    pub oidc_enabled: bool,

    pub oidc_config: Option<OidcConfig>,
    #[serde(default)]
    pub tls_enabled: bool,

    #[serde(default)]
    pub cert_file: Option<PathBuf>,
    #[serde(default)]
    pub cert_key: Option<PathBuf>,

    #[serde(default = "default_max_concurrent_checks")]
    pub max_concurrent_checks: usize,
}

impl Configuration {
    pub async fn new(filename: &PathBuf) -> Result<Self, Error> {
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
            res.hosts.insert(
                LOCAL_SERVICE_HOST_NAME.to_string(),
                Host::new(LOCAL_SERVICE_HOST_NAME.to_string(), HostCheck::None),
            );
        }
        Ok(res)
    }

    #[cfg(test)]
    pub async fn load_test_config() -> Arc<Self> {
        let mut res: Configuration = serde_json::from_str(
            &tokio::fs::read_to_string("maremma.example.json")
                .await
                .expect("Failed to read example config"),
        )
        .expect("Failed to parse example config");

        if !res.local_services.services.is_empty() {
            res.hosts.insert(
                LOCAL_SERVICE_HOST_NAME.to_string(),
                Host::new(LOCAL_SERVICE_HOST_NAME.to_string(), HostCheck::None),
            );
        }
        Arc::new(res)
    }

    pub fn frontend_url(&self) -> String {
        self.frontend_url.clone().unwrap_or_else(|| {
            let proto = if self.tls_enabled { "https" } else { "http" };
            let port = match self.listen_port {
                Some(port) => {
                    if [80, 443].contains(&port) {
                        "".to_string()
                    } else {
                        port.to_string()
                    }
                }
                None => crate::constants::DEFAULT_PORT.to_string(),
            };
            format!("{}://{}:{}", proto, self.listen_address, port)
        })
    }

    // Pulls the groups from hosts and services in the config
    pub fn groups(&self) -> Vec<String> {
        let mut groups: HashSet<String> = HashSet::new();

        self.hosts.values().for_each(|host| {
            host.host_groups.iter().cloned().for_each(|group| {
                groups.insert(group);
            });
        });

        if let Some(services) = &self.services {
            if let Some(services) = services.as_object() {
                services.iter().for_each(|(_service_name, service)| {
                    if let Some(service) = service.as_object() {
                        if let Some(host_groups) = service.get("host_groups") {
                            if let Some(group_values) = host_groups.as_array() {
                                group_values.iter().for_each(|group| {
                                    if let Some(group) = group.as_str() {
                                        groups.insert(group.to_string());
                                    }
                                });
                            }
                        }
                    }
                });
            }
        }

        groups.into_iter().collect()
    }

    pub fn prune(&self, _db: &DatabaseConnection) -> Result<(), Error> {
        // check the hosts agsinst the config file

        // check the groups against the config file

        // check the services against the config file

        // check the checks against the config file

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{default_max_concurrent_checks, Configuration};
    use crate::db::tests::test_setup;

    #[tokio::test]
    async fn test_config_new() {
        assert!(Configuration::new(
            &"asdfsdafdsf.asdfsadfdf"
                .parse()
                .expect("Failed to parse filename")
        )
        .await
        .is_err());

        let config = serde_json::json! {{
            "hosts": {
                "foo.bar" : {
                    "hostname" : "foo.bar"
                }
            }
        }}
        .to_string();
        let config = Configuration::new_from_string(&config).await.unwrap();
        assert_eq!(config.hosts.len(), 1);
    }

    #[tokio::test]
    async fn test_config_groups() {
        let (_db, config) = test_setup().await.expect("Failed to setup test");

        let groups = config.groups();
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn test_default_max_concurrent_checks() {
        assert!(default_max_concurrent_checks() >= 1);
    }
}
