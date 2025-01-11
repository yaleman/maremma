//! Configuration handling for Maremma

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU16;
use std::path::PathBuf;

use schemars::JsonSchema;

use crate::constants::{
    web_server_default_port, DEFAULT_SERVICE_CHECK_HISTORY_STORAGE, WEB_SERVER_DEFAULT_STATIC_PATH,
};
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
/// Parses configuration from the file
pub struct ConfigurationParser {
    #[serde(default = "default_database_file")]
    /// Path to the database file (or `:memory:` for in-memory)
    pub database_file: String,

    /// The path to the web server's static files, defaults to [crate::constants::WEB_SERVER_DEFAULT_STATIC_PATH]
    pub static_path: Option<PathBuf>,

    #[serde(default = "default_listen_address")]
    /// The listen address, eg `0.0.0.0` or `127.0.0.1`
    pub listen_address: String,

    /// Defaults to 8888
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listen_port: Option<NonZeroU16>,

    /// Target host configuration
    pub hosts: HashMap<String, Host>,

    #[serde(default)]
    /// Services to run locally
    pub local_services: FakeHost,

    #[serde(skip_serializing, default)]
    /// Service configuration
    pub services: HashMap<String, Value>,

    /// The frontend URL ie `https://maremma.example.com` used for things like OIDC
    pub frontend_url: Option<String>,
    /// OIDC issuer (url)
    pub oidc_issuer: Option<String>,
    /// OIDC client_id
    pub oidc_client_id: Option<String>,
    /// OIDC client_secret
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_secret: Option<String>,

    #[serde(default)]
    /// The path to the TLS certificate
    pub cert_file: PathBuf,
    #[serde(default)]
    /// The path to the TLS key
    pub cert_key: PathBuf,

    #[serde(default = "default_max_concurrent_checks")]
    /// The maximum concurrent checks we'll run at one time
    pub max_concurrent_checks: usize,

    /// How many history entries to keep per check, defaults to 25000 ([crate::constants::DEFAULT_HISTORY_LIMIT]), setting this too high can cause slowdowns.
    pub max_history_entries_per_check: Option<u64>,
}

/// A sendable configuration, for use across threads
pub type SendableConfig = Arc<RwLock<Configuration>>;

#[derive(Serialize, Deserialize, Debug, Default, JsonSchema)]
/// The result of parsing the configuration file, don't instantiate this directly!
pub struct Configuration {
    #[serde(default = "default_database_file")]
    /// Path to the database file (or `:memory:` for in-memory)
    pub database_file: String,

    /// The path to the web server's static files, defaults to [crate::constants::WEB_SERVER_DEFAULT_STATIC_PATH]
    pub static_path: Option<PathBuf>,

    #[serde(default = "default_listen_address")]
    /// The listen address, eg `0.0.0.0` or `127.0.0.1``
    pub listen_address: String,

    /// Defaults to 8888
    pub listen_port: Option<NonZeroU16>,

    /// Host configuration
    pub hosts: HashMap<String, Host>,

    #[serde(default)]
    /// Services to run locally
    pub local_services: FakeHost,

    /// Service configuration
    #[serde(default)]
    pub services: HashMap<String, Service>,

    /// The frontend URL ie `https://maremma.example.com` used for things like OIDC
    pub frontend_url: String,

    /// OIDC issuer (url)
    pub oidc_issuer: String,
    /// OIDC client_id
    pub oidc_client_id: String,
    /// OIDC client_secret
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_secret: Option<String>,

    /// the TLS certificate matter
    pub cert_file: PathBuf,
    /// the TLS certificate matter
    pub cert_key: PathBuf,

    #[serde(default = "default_max_concurrent_checks")]
    /// The maximum concurrent checks we'll run at one time
    pub max_concurrent_checks: usize,

    /// How many history entries to keep per check, defaults to 25000 ([crate::constants::DEFAULT_HISTORY_LIMIT]), setting this too high can cause slowdowns.
    pub(crate) max_history_entries_per_check: u64,
}

impl TryFrom<ConfigurationParser> for Configuration {
    fn try_from(value: ConfigurationParser) -> Result<Self, Error> {
        let services = value
            .services
            .iter()
            .map(|(name, service)| {
                let service: Service = serde_json::from_value(service.clone()).map_err(|e| {
                    Error::Configuration(format!("Failed to parse service {}: {}", name, e))
                })?;
                Ok((name.clone(), service))
            })
            .collect::<Result<HashMap<String, Service>, Error>>()?;

        let static_path = value
            .static_path
            .unwrap_or(PathBuf::from(WEB_SERVER_DEFAULT_STATIC_PATH));

        if !static_path.exists() {
            return Err(Error::Configuration(
                "Static path does not exist".to_string(),
            ));
        }

        let listen_port: Option<NonZeroU16> = value
            .listen_port
            .map(|lp| {
                NonZeroU16::try_from(lp).map_err(|_| {
                    Error::Configuration("Failed to convert listen port to NonZeroU16".to_string())
                })
            })
            .transpose()?;
        let frontend_url = match value.frontend_url {
            Some(val) => val,
            None => match std::env::var("MAREMMA_FRONTEND_URL") {
                Ok(val) => val,
                Err(_) => return Err(Error::Configuration("Frontend URL not set".to_string())),
            },
        };
        let oidc_issuer = match value.oidc_issuer {
            Some(val) => val,
            None => match std::env::var("MAREMMA_OIDC_ISSUER") {
                Ok(val) => val,
                Err(_) => return Err(Error::Configuration("OIDC Issuer URL not set".to_string())),
            },
        };

        let oidc_client_id = match value.oidc_client_id {
            Some(val) => val,
            None => match std::env::var("MAREMMA_OIDC_CLIENT_ID") {
                Ok(val) => val,
                Err(_) => return Err(Error::Configuration("OIDC Client ID not set".to_string())),
            },
        };

        Ok(Configuration {
            database_file: value.database_file,
            listen_address: value.listen_address,
            listen_port,
            hosts: value.hosts,
            local_services: value.local_services,
            services,
            frontend_url,
            oidc_issuer,
            oidc_client_id,
            oidc_client_secret: value.oidc_client_secret,

            cert_file: value.cert_file,
            cert_key: value.cert_key,
            max_concurrent_checks: value.max_concurrent_checks,
            static_path: Some(static_path),
            max_history_entries_per_check: value
                .max_history_entries_per_check
                .unwrap_or(DEFAULT_SERVICE_CHECK_HISTORY_STORAGE),
        })
    }

    type Error = Error;
}

impl Configuration {
    /// New Configuration object from a file reference
    pub async fn new(filename: &PathBuf) -> Result<Self, Error> {
        if !filename.exists() {
            return Err(Error::ConfigFileNotFound(
                filename.to_string_lossy().to_string(),
            ));
        }
        debug!("Loading config from {:?}", filename);
        Self::new_from_string(&tokio::fs::read_to_string(filename).await?).await
    }

    /// If you've got the file contents, use that to build a configuration
    pub async fn new_from_string(config: &str) -> Result<Self, Error> {
        let mut res: ConfigurationParser = serde_json::from_str(config)?;

        if !res.local_services.services.is_empty() {
            res.hosts.insert(
                LOCAL_SERVICE_HOST_NAME.to_string(),
                Host::new(LOCAL_SERVICE_HOST_NAME.to_string(), HostCheck::None),
            );
        }

        res.try_into()
    }

    #[cfg(test)]
    /// Loads a bare test config
    pub async fn load_test_config_bare() -> Self {
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
        res.try_into().expect("Failed to convert test config")
    }

    #[cfg(test)]
    /// Loads a test config
    pub async fn load_test_config() -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Self::load_test_config_bare().await))
    }

    /// returns the listen address and port as a string ie `127.0.0.1:8888`
    pub fn listen_addr(&self) -> String {
        format!(
            "{}:{}",
            self.listen_address,
            self.listen_port.unwrap_or(web_server_default_port())
        )
    }

    /// Pulls the groups from hosts and services in the config
    pub fn groups(&self) -> Vec<String> {
        let mut groups: HashSet<String> = HashSet::new();

        self.hosts.values().for_each(|host| {
            host.host_groups.iter().cloned().for_each(|group| {
                groups.insert(group);
            });
        });

        self.services.iter().for_each(|(_service_name, service)| {
            groups.extend(service.host_groups.iter().cloned());
        });

        groups.into_iter().collect()
    }

    /// Prune the configuration based on the database, so we can serialize it back
    pub async fn prune(&mut self, db: Arc<RwLock<DatabaseConnection>>) -> Result<(), Error> {
        // TODO: prune config

        let db_writer = db.write().await;

        // check the hosts against the config file
        let db_hosts = entities::host::Entity::find().all(&*db_writer).await?;
        let config_hosts = self.hosts.keys().cloned().collect::<HashSet<String>>();

        // keep a record of the ones we find in the db
        let mut db_host_names = HashMap::new();

        for db_host in db_hosts {
            debug!("Host: {:?}", db_host);
            if !config_hosts.contains(&db_host.name) {
                warn!("Need to add Host {} to config", db_host.name);
            }
            db_host_names.insert(db_host.name, db_host.id);
        }

        // check the groups against the config file
        let db_host_groups = entities::host_group::Entity::find()
            .all(&*db_writer)
            .await?;
        let config_groups = self.groups();
        for host_group in db_host_groups {
            debug!("HostGroup: {:?}", host_group);
            if !config_groups.contains(&host_group.name) {
                warn!("Need to add group {} to config", host_group.name);
            }
        }

        // check the services against the config file
        let db_services = entities::service::Entity::find().all(&*db_writer).await?;
        let config_services = self.services.keys().cloned().collect::<HashSet<String>>();
        for service in db_services {
            debug!("Service: {:?}", service);
            if !config_services.contains(&service.name) {
                warn!("Service {} not in config", service.name);
            }
        }

        // check the checks against the config file
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{default_max_concurrent_checks, Configuration};
    use crate::db::tests::test_setup;

    use schemars::schema_for;

    use super::ConfigurationParser;
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
            },
            "frontend_url": "https://example.com",
            "oidc_issuer" : "https://example.com",
            "oidc_client_id" : "foo",
            "oidc_client_secret" : "bar",
        }}
        .to_string();
        let config = Configuration::new_from_string(&config).await.unwrap();
        assert_eq!(config.hosts.len(), 1);

        assert_eq!(config.listen_addr(), "127.0.0.1:8888");
    }

    #[tokio::test]
    async fn test_config_groups() {
        let (_db, config) = test_setup().await.expect("Failed to setup test");

        for group in config.read().await.groups() {
            assert!(!group.is_empty());
        }
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

    #[test]
    // This tries setting a static path that shouldn't exist, so it can throw an error
    fn test_config_static_missing() {
        let mut cfg = ConfigurationParser::default();

        cfg.static_path = Some("/tmp/does-not-exist".parse().unwrap());
        assert!(Configuration::try_from(cfg).is_err());
    }

    #[tokio::test]
    async fn test_config_prune() {
        let (db, config) = test_setup().await.expect("Failed to setup test");

        config
            .write()
            .await
            .prune(db)
            .await
            .expect("Failed to prune config");
    }
}
