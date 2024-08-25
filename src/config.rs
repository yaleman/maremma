//! Configuration handling for Maremma

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use reqwest::Url;
use schemars::JsonSchema;

use crate::constants::WEB_SERVER_DEFAULT_STATIC_PATH;
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
/// OIDC Config
pub struct OidcConfig {
    /// OIDC issuer (url)
    pub issuer: String,
    /// OIDC client_id
    pub client_id: String,
    /// OIDC client_secret
    pub client_secret: Option<String>,
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
    pub listen_port: Option<u16>,

    /// Target host configuration
    pub hosts: HashMap<String, Host>,

    #[serde(default)]
    /// Services to run locally
    pub local_services: FakeHost,

    #[serde(skip_serializing)]
    /// Service configuration
    pub services: Option<HashMap<String, Value>>,

    /// The frontend URL ie `https://maremma.example.com` used for things like OIDC
    pub frontend_url: Option<String>,

    #[serde(default)]
    /// Should we enable OIDC authentication?
    pub oidc_enabled: bool,

    /// OIDC configuration, see [OidcConfig]
    pub oidc_config: Option<OidcConfig>,

    #[serde(default)]
    /// The path to the TLS certificate
    pub cert_file: PathBuf,
    #[serde(default)]
    /// The path to the TLS key
    pub cert_key: PathBuf,

    #[serde(default = "default_max_concurrent_checks")]
    /// The maximum concurrent checks we'll run at one time
    pub max_concurrent_checks: usize,
}

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
    pub listen_port: Option<u16>,

    /// Host configuration
    pub hosts: HashMap<String, Host>,

    #[serde(default)]
    /// Services to run locally
    pub local_services: FakeHost,

    /// Service configuration
    pub services: Option<HashMap<String, Service>>,

    /// The frontend URL ie `https://maremma.example.com` used for things like OIDC
    pub frontend_url: Option<String>,

    #[serde(default)]
    /// Should we enable OIDC authentication?
    pub oidc_enabled: bool,

    /// OIDC configuration, see [OidcConfig]
    pub oidc_config: Option<OidcConfig>,

    /// the TLS certificate matter
    pub cert_file: PathBuf,
    /// the TLS certificate matter
    pub cert_key: PathBuf,

    #[serde(default = "default_max_concurrent_checks")]
    /// The maximum concurrent checks we'll run at one time
    pub max_concurrent_checks: usize,
}

impl TryFrom<ConfigurationParser> for Configuration {
    fn try_from(value: ConfigurationParser) -> Result<Self, Error> {
        let services = match value.services {
            Some(services) => {
                let mut res: HashMap<String, Service> = HashMap::new();
                for (service_name, service) in services {
                    let service: Service = serde_json::from_value(service)?;

                    res.insert(service_name, service);
                }
                Some(res)
            }
            None => None,
        };

        let static_path = value
            .static_path
            .unwrap_or(PathBuf::from(WEB_SERVER_DEFAULT_STATIC_PATH));

        if !static_path.exists() {
            return Err(Error::Configuration(
                "Static path does not exist".to_string(),
            ));
        }

        // handle the case where the frontend URL is set but doesn't start with https
        if let Some(url) = &value.frontend_url {
            // parse the URL
            let url = Url::parse(url).map_err(|e| {
                Error::Configuration(format!("Failed to parse frontend url: {}", e))
            })?;

            if url.scheme() != "https" {
                return Err(Error::Configuration(
                    "Frontend URL must start with https".to_string(),
                ));
            }
        }

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
            static_path: Some(static_path),
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

    /// Getter for the frontend URL
    pub fn frontend_url(&self) -> String {
        self.frontend_url.clone().unwrap_or_else(|| {
            let port = self
                .listen_port
                .unwrap_or(crate::constants::WEB_SERVER_DEFAULT_PORT);
            format!("https://{}:{}", self.listen_address, port)
        })
    }

    /// returns the listen address and port as a string ie `127.0.0.1:8888`
    pub fn listen_addr(&self) -> String {
        format!(
            "{}:{}",
            self.listen_address,
            self.listen_port
                .unwrap_or(crate::constants::WEB_SERVER_DEFAULT_PORT)
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

        if let Some(services) = &self.services {
            services.iter().for_each(|(_service_name, service)| {
                groups.extend(service.host_groups.iter().cloned());
            });
        }

        groups.into_iter().collect()
    }

    /// Prune the configuration based on the database, so we can serialize it back
    pub fn prune(&self, _db: &DatabaseConnection) -> Result<(), Error> {
        // check the hosts against the config file

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
            }
        }}
        .to_string();
        let config = Configuration::new_from_string(&config).await.unwrap();
        assert_eq!(config.hosts.len(), 1);

        assert_eq!(config.listen_addr(), "127.0.0.1:8888");
    }

    #[tokio::test]
    async fn test_config_groups() {
        let (_db, config) = test_setup().await.expect("Failed to setup test");

        for group in config.groups() {
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

    #[test]
    // Testing when the config has incorrect frontend URLs
    fn test_config_invalid_frontend() {
        let mut cfg = ConfigurationParser::default();

        cfg.frontend_url = Some("http://example.com".to_string());
        assert!(Configuration::try_from(cfg).is_err());
        let mut cfg = ConfigurationParser::default();

        cfg.frontend_url = Some("ftp://example.com".to_string());
        assert!(Configuration::try_from(cfg).is_err());
    }
}
