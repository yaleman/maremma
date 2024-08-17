use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use schemars::JsonSchema;

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

#[derive(Serialize, Deserialize, Debug, Default, JsonSchema)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ConfigurationParser {
    #[serde(default = "default_database_file")]
    pub database_file: String,

    /// The path to the web server's static files, defaults to ./static
    pub static_path: Option<PathBuf>,

    #[serde(default = "default_listen_address")]
    pub listen_address: String,

    //// Defaults to 8888
    pub listen_port: Option<u16>,

    pub hosts: HashMap<String, Host>,

    #[serde(default)]
    pub local_services: FakeHost,

    // This is something we need to deserialize later because it's messy
    #[serde(skip_serializing)]
    pub services: Option<HashMap<String, Value>>,

    /// The frontend URL ie `https://maremma.example.com` used for things like OIDC
    pub frontend_url: Option<String>,

    #[serde(default)]
    pub oidc_enabled: bool,

    pub oidc_config: Option<OidcConfig>,

    #[serde(default)]
    pub cert_file: Option<PathBuf>,
    #[serde(default)]
    pub cert_key: Option<PathBuf>,

    #[serde(default = "default_max_concurrent_checks")]
    pub max_concurrent_checks: usize,
}

#[derive(Serialize, Deserialize, Debug, Default, JsonSchema)]
pub struct Configuration {
    #[serde(default = "default_database_file")]
    pub database_file: String,

    /// The path to the web server's static files, defaults to ./static
    pub static_path: PathBuf,

    #[serde(default = "default_listen_address")]
    pub listen_address: String,

    //// Defaults to 8888
    pub listen_port: Option<u16>,

    pub hosts: HashMap<String, Host>,

    #[serde(default)]
    pub local_services: FakeHost,

    // This is something we need to deserialize later because it's messy
    pub services: Option<HashMap<String, Service>>,

    /// The frontend URL ie `https://maremma.example.com` used for things like OIDC
    pub frontend_url: Option<String>,

    #[serde(default)]
    pub oidc_enabled: bool,

    pub oidc_config: Option<OidcConfig>,

    #[serde(default)]
    pub cert_file: Option<PathBuf>,
    #[serde(default)]
    pub cert_key: Option<PathBuf>,

    #[serde(default = "default_max_concurrent_checks")]
    pub max_concurrent_checks: usize,
}

impl TryFrom<ConfigurationParser> for Configuration {
    fn try_from(value: ConfigurationParser) -> Result<Self, Error> {
        let services = match value.services {
            Some(services) => {
                let mut res: HashMap<String, Service> = HashMap::new();
                for (service_name, service) in services {
                    res.insert(service_name, serde_json::from_value(service)?);
                }
                Some(res)
            }
            None => None,
        };

        Ok(Configuration {
            database_file: value.database_file,
            listen_address: value.listen_address,
            listen_port: value.listen_port,
            hosts: value.hosts,
            local_services: value.local_services,
            services,
            frontend_url: value.frontend_url,
            oidc_enabled: value.oidc_enabled,
            oidc_config: value.oidc_config,
            cert_file: value.cert_file,
            cert_key: value.cert_key,
            max_concurrent_checks: value.max_concurrent_checks,
            static_path: value.static_path.unwrap_or(PathBuf::from("./static")),
        })
    }

    type Error = Error;
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
        let mut res: ConfigurationParser = serde_json::from_str(config)?;

        if !res.local_services.services.is_empty() {
            res.hosts.insert(
                LOCAL_SERVICE_HOST_NAME.to_string(),
                Host::new(LOCAL_SERVICE_HOST_NAME.to_string(), HostCheck::None),
            );
        }

        // handle the case where the frontend URL is set but doesn't start with https
        if let Some(url) = &res.frontend_url {
            if !url.starts_with("https") {
                return Err(Error::Configuration(
                    "Frontend URL must start with https".to_string(),
                ));
            }
        }
        res.try_into()
    }

    #[cfg(test)]
    pub async fn load_test_config() -> Arc<Self> {
        let mut res: ConfigurationParser = serde_json::from_str(
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
        let res: Configuration = res.try_into().expect("Failed to convert test config");
        Arc::new(res)
    }

    pub fn frontend_url(&self) -> String {
        self.frontend_url.clone().unwrap_or_else(|| {
            let port = self.listen_port.unwrap_or(crate::constants::DEFAULT_PORT);
            format!("https://{}:{}", self.listen_address, port)
        })
    }

    // returns the listen address and port as a string ie `127.0.0.1:8888`
    pub fn listen_addr(&self) -> String {
        format!(
            "{}:{}",
            self.listen_address,
            self.listen_port.unwrap_or(crate::constants::DEFAULT_PORT)
        )
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
            services.iter().for_each(|(_service_name, service)| {
                groups.extend(service.host_groups.iter().cloned());
            });
        }

        groups.into_iter().collect()
    }

    pub fn prune(&self, _db: &DatabaseConnection) -> Result<(), Error> {
        // check the hosts agsinst the config file

        // check the groups against the config file

        // check the services against the config file

        // check the checks against the config file
        // TODO: prune config
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{default_max_concurrent_checks, Configuration};
    use crate::db::tests::test_setup;

    use schemars::schema_for;
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

    #[test]
    fn test_json_schema() {
        let schema = schema_for!(Configuration);

        println!("{}", serde_json::to_string_pretty(&schema).unwrap());
    }
}
