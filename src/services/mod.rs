//! Service check implementations

pub mod cli;
pub mod http;
pub mod kubernetes;
pub mod oneshot;
pub mod ping;
pub mod ssh;
pub mod tls;

use crate::check_loop::CheckResult;
use crate::db::entities;
use crate::prelude::*;
use std::fmt::{self, Debug, Display, Formatter};

use clap::ValueEnum;
use sea_orm::{sea_query, DeriveActiveEnum, EnumIter, Iden};
use serde::de::DeserializeOwned;

use crate::errors::Error;
#[derive(
    Deserialize, Debug, Serialize, PartialEq, Eq, Copy, Clone, DeriveActiveEnum, EnumIter, Iden,
)]
#[serde(rename_all = "lowercase")]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(16))")]
/// The result of a service check
#[allow(missing_docs)]
pub enum ServiceStatus {
    #[sea_orm(string_value = "ok")]
    Ok,
    #[sea_orm(string_value = "pending")]
    Pending,
    #[sea_orm(string_value = "critical")]
    Critical,
    #[sea_orm(string_value = "checking")]
    Checking,
    #[sea_orm(string_value = "warning")]
    Warning,
    #[sea_orm(string_value = "error")]
    Error,
    #[sea_orm(string_value = "unknown")]
    Unknown,
    /// Run this as soon as possible
    #[sea_orm(string_value = "urgent")]
    Urgent,
    #[sea_orm(string_value = "disabled")]
    Disabled,
}

impl Display for ServiceStatus {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            format!("{:?}", self)
                .split(':')
                .last()
                .unwrap_or(format!("{:?}", self).as_str()) // should never trigger this
        )
    }
}

impl Default for ServiceStatus {
    fn default() -> Self {
        Self::Pending
    }
}

impl ServiceStatus {
    /// Returns the cell background colour for the status, from the [bootstrap colours](https://getbootstrap.com/docs/5.3/utilities/colors/)
    pub fn as_html_class_background(self) -> &'static str {
        match self {
            ServiceStatus::Ok => "success",
            ServiceStatus::Critical | ServiceStatus::Error => "danger",
            ServiceStatus::Checking | ServiceStatus::Warning => "warning",
            ServiceStatus::Pending | ServiceStatus::Disabled | ServiceStatus::Unknown => {
                "secondary"
            }
            ServiceStatus::Urgent => "primary",
        }
    }

    /// Returns the text colour for the status, from the [bootstrap colours](https://getbootstrap.com/docs/5.3/utilities/colors/)
    pub fn as_html_class_text(self) -> &'static str {
        match self {
            ServiceStatus::Ok => "light",
            ServiceStatus::Critical | ServiceStatus::Error => "dark",
            ServiceStatus::Checking | ServiceStatus::Warning => "light",
            ServiceStatus::Pending | ServiceStatus::Disabled | ServiceStatus::Unknown => "dark",
            ServiceStatus::Urgent => "light",
        }
    }
}

#[async_trait]
/// The base trait for a service
pub trait ServiceTrait: Debug + Sync + Send {
    /// Run the service check
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error>;

    /// Validate the configuration against some extra rules
    fn validate(&self) -> Result<(), Error> {
        debug!("You're using the default always-ok validation for this service");
        Ok(())
    }

    /// Parse it from the configuration file
    fn from_config(config: &Value) -> Result<Self, Error>
    where
        Self: Sized + DeserializeOwned,
    {
        serde_json::from_value(config.clone()).map_err(Error::from)
    }
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
/// Base service type
pub struct Service {
    #[serde(default = "uuid::Uuid::new_v4")]
    #[schemars(
        default,
        description = "The internal ID of the service, regenerated internally if not provided"
    )]
    /// The internal ID of the service
    pub id: Uuid,
    /// This is pulled from the config file's key
    pub name: Option<String>,
    /// Description of the service
    pub description: Option<String>,
    /// Host groups to apply it to
    pub host_groups: Vec<String>,

    /// What kind of service it is
    pub service_type: ServiceType,
    #[serde(with = "crate::serde::cron")]
    #[schemars(with = "String")]
    /// Cron schedule for the service, eg `@hourly`, `* * * * * *` or `0 0 * * *`
    pub cron_schedule: Cron,

    /// Catch-all for the other fields in the config
    #[serde(flatten)]
    pub extra_config: HashMap<String, Value>,

    #[serde(skip)]
    /// Internal configuration storage, don't specify this
    pub config: Option<Box<dyn ServiceTrait>>,
}

pub(crate) fn service_config_parse(
    service_identifier: &str,
    service_type: &ServiceType,
    value: &Value,
) -> Result<Box<dyn ServiceTrait>, Error> {
    let res = match service_type {
        ServiceType::Cli => Box::new(
            cli::CliService::from_config(value)
                .inspect_err(|_| error!("Failed to parse config for {}", service_identifier))?,
        ) as Box<dyn ServiceTrait>,
        ServiceType::Ssh => Box::new(
            ssh::SshService::from_config(value)
                .inspect_err(|_| error!("Failed to parse config for {}", service_identifier))?,
        ) as Box<dyn ServiceTrait>,
        ServiceType::Ping => Box::new(
            ping::PingService::from_config(value)
                .inspect_err(|_| error!("Failed to parse config for {}", service_identifier))?,
        ) as Box<dyn ServiceTrait>,
        ServiceType::Http => Box::new(
            http::HttpService::from_config(value)
                .inspect_err(|_| error!("Failed to parse config for {}", service_identifier))?,
        ) as Box<dyn ServiceTrait>,
        ServiceType::Tls => Box::new(
            tls::TlsService::from_config(value)
                .inspect_err(|_| error!("Failed to parse config for {}", service_identifier))?,
        ) as Box<dyn ServiceTrait>,
    };

    res.validate()?;

    Ok(res)
}

impl Service {
    /// Because services are stored in the database as a JSON field, we need to parse the config and store the type internally
    pub fn parse_config(&mut self) -> Result<Self, Error> {
        let value = serde_json::to_value(&*self)?;

        let service_identifier = match &self.name {
            Some(name) => name.clone(),
            None => self.id.hyphenated().to_string(),
        };

        let config = service_config_parse(&service_identifier, &self.service_type, &value)?;

        Ok(Self {
            id: self.id,
            name: self.name.to_owned(),
            description: self.description.to_owned(),
            host_groups: self.host_groups.to_owned(),
            service_type: self.service_type.to_owned(),
            cron_schedule: self.cron_schedule.to_owned(),
            extra_config: self.extra_config.to_owned(),
            config: Some(config),
        })
    }
}

impl TryFrom<&Value> for Service {
    type Error = Error;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        let mut res: Service = serde_json::from_value(value.clone())?;
        res.parse_config()
    }
}

impl TryFrom<&entities::service::Model> for Service {
    type Error = Error;

    fn try_from(value: &entities::service::Model) -> Result<Self, Self::Error> {
        let host_groups = match &value.host_groups.is_array() {
            false => {
                debug!("No host groups in service {}", value.name);
                vec![]
            }
            true => serde_json::from_value(value.host_groups.clone())?,
        };

        let extra_config = match value.extra_config.clone() {
            None => {
                debug!("No extra config in service {}", value.name);
                HashMap::new()
            }
            Some(extra_config) => serde_json::from_value(extra_config)?,
        };

        let service = Service {
            id: value.id,
            name: Some(value.name.clone()),
            description: value.description.clone(),
            host_groups,
            service_type: value.service_type.clone(),
            cron_schedule: Cron::new(&value.cron_schedule).parse()?,
            extra_config,
            config: None,
        }
        .parse_config()?;

        Ok(service)
    }
}

#[derive(
    Deserialize,
    Debug,
    Serialize,
    PartialEq,
    Eq,
    Clone,
    DeriveActiveEnum,
    EnumIter,
    Iden,
    JsonSchema,
    ValueEnum,
)]
#[serde(rename_all = "lowercase")]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(5))")]
/// The type of service
pub enum ServiceType {
    /// CLI service
    #[sea_orm(string_value = "cli")]
    Cli,
    /// SSH service
    #[sea_orm(string_value = "ssh")]
    Ssh,
    /// Ping service
    #[sea_orm(string_value = "ping")]
    Ping,
    /// HTTP service
    #[sea_orm(string_value = "http")]
    Http,
    /// TLS service
    #[sea_orm(string_value = "tls")]
    Tls,
}

impl Display for ServiceType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Cli => write!(f, "CLI"),
            Self::Ssh => write!(f, "SSH"),
            Self::Ping => write!(f, "Ping"),
            Self::Http => write!(f, "HTTP"),
            Self::Tls => write!(f, "TLS"),
        }
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::Iterable;

    use crate::db::tests::test_setup;
    use crate::prelude::*;

    use super::*;

    #[test]
    fn test_servicestatus_display() {
        for status in ServiceStatus::iter() {
            assert_eq!(
                format!("{}", status),
                format!("{:?}", status)
                    .split(':')
                    .last()
                    .expect("This should be impossible to fail")
            );
        }
    }

    #[test]
    fn test_servicestatus_as_html_class_background() {
        assert_eq!(ServiceStatus::Ok.as_html_class_background(), "success");
        assert_eq!(ServiceStatus::Critical.as_html_class_background(), "danger");
        assert_eq!(
            ServiceStatus::Checking.as_html_class_background(),
            "warning"
        );
        assert_eq!(
            ServiceStatus::Pending.as_html_class_background(),
            "secondary"
        );
        assert_eq!(
            ServiceStatus::Disabled.as_html_class_background(),
            "secondary"
        );
        assert_eq!(
            ServiceStatus::Unknown.as_html_class_background(),
            "secondary"
        );
        assert_eq!(ServiceStatus::Urgent.as_html_class_background(), "primary");
    }

    #[test]
    fn test_servicestatus_as_html_class_text() {
        assert_eq!(ServiceStatus::Ok.as_html_class_text(), "light");
        assert_eq!(ServiceStatus::Critical.as_html_class_text(), "dark");
        assert_eq!(ServiceStatus::Checking.as_html_class_text(), "light");
        assert_eq!(ServiceStatus::Pending.as_html_class_text(), "dark");
        assert_eq!(ServiceStatus::Disabled.as_html_class_text(), "dark");
        assert_eq!(ServiceStatus::Unknown.as_html_class_text(), "dark");
        assert_eq!(ServiceStatus::Urgent.as_html_class_text(), "light");
    }

    #[tokio::test]
    /// iterate through a bunch of different conversions
    async fn test_service_from_model() {
        let (db, _config) = test_setup()
            .await
            .expect("Failed to set up test environment");

        let service = entities::service::Entity::find()
            .filter(entities::service::Column::ServiceType.eq(ServiceType::Ping))
            .one(db.as_ref())
            .await
            .unwrap()
            .unwrap();

        let _service_from_model =
            Service::try_from(&service).expect("Failed to convert model to service");

        let service_without_host_groups = entities::service::Model {
            host_groups: Default::default(),
            service_type: ServiceType::Ping,
            extra_config: None,
            ..service.clone()
        };

        let service_without_host_groups_model = Service::try_from(&service_without_host_groups)
            .expect("Failed to take service without groups from model");
        assert!(service_without_host_groups_model.host_groups.is_empty());

        let service_as_value =
            serde_json::to_value(&service).expect("Failed to convert service model to value");
        debug!("Service as value: {:?}", service_as_value);
        let _service_from_value: Service = Service::try_from(&service_as_value)
            .expect("Failed to convert value to service")
            .parse_config()
            .expect("Failed to parse config");
    }

    #[test]
    fn test_display_service_type() {
        assert_eq!(format!("{}", ServiceType::Cli), "CLI");
        assert_eq!(format!("{}", ServiceType::Ssh), "SSH");
        assert_eq!(format!("{}", ServiceType::Ping), "Ping");
        assert_eq!(format!("{}", ServiceType::Http), "HTTP");
        assert_eq!(format!("{}", ServiceType::Tls), "TLS");
    }

    #[test]
    fn test_parse_http_service_configs() {
        let config = r#"{
            "name": "test",
            "service_type": "http",
            "host_groups": ["test"],
            "http_uri" : "/foo",
            "http_method" : "POST",
            "cron_schedule": "@hourly"
        }"#;
        let value: Value = serde_json::from_str(config).expect("Failed to parse config");
        let service = Service::try_from(&value).expect("Failed to parse service");
        assert_eq!(service.name, Some("test".to_string()));
        assert_eq!(service.service_type, ServiceType::Http);
        assert_eq!(service.host_groups, vec!["test".to_string()]);
        assert_eq!(
            service.cron_schedule.pattern.to_string(),
            Cron::new("@hourly").parse().unwrap().pattern.to_string()
        );
    }

    #[test]
    fn test_parse_cli_service_config() {
        let config = r#"{
            "name": "test",
            "service_type": "cli",
            "host_groups": ["test"],
            "command_line": "ls -lah .",
            "cron_schedule": "@hourly"
        }"#;
        let value: Value = serde_json::from_str(config).expect("Failed to parse config");
        let service = Service::try_from(&value).expect("Failed to parse service");
        assert_eq!(service.name, Some("test".to_string()));
        assert_eq!(service.service_type, ServiceType::Cli);
        assert_eq!(service.host_groups, vec!["test".to_string()]);
        assert_eq!(
            service.cron_schedule.pattern.to_string(),
            Cron::new("@hourly").parse().unwrap().pattern.to_string()
        );
    }

    #[test]
    fn test_parse_ssh_service_config() {
        let config = r#"{
            "name": "test",
            "service_type": "ssh",
            "host_groups": ["test"],
            "command_line": "ls -lah .",
            "cron_schedule": "@hourly",
            "username" : "test",
            "password" : "oh no this isn't a password!"
        }"#;
        let value: Value = serde_json::from_str(config).expect("Failed to parse config");
        let service = Service::try_from(&value).expect("Failed to parse service");
        assert_eq!(service.name, Some("test".to_string()));
        assert_eq!(service.service_type, ServiceType::Ssh);
        assert_eq!(service.host_groups, vec!["test".to_string()]);
        assert_eq!(
            service.cron_schedule.pattern.to_string(),
            Cron::new("@hourly").parse().unwrap().pattern.to_string()
        );
    }

    #[test]
    fn test_parse_ping_service_config() {
        let config = r#"{
            "name": "test",
            "service_type": "ping",
            "host_groups": ["test"],
            "cron_schedule": "@hourly"
        }"#;
        let value: Value = serde_json::from_str(config).expect("Failed to parse config");
        let service = Service::try_from(&value).expect("Failed to parse service");
        assert_eq!(service.name, Some("test".to_string()));
        assert_eq!(service.service_type, ServiceType::Ping);
        assert_eq!(service.host_groups, vec!["test".to_string()]);
        assert_eq!(
            service.cron_schedule.pattern.to_string(),
            Cron::new("@hourly").parse().unwrap().pattern.to_string()
        );
    }

    #[test]
    fn test_parse_service_from_value() {
        let config = r#"{
            "name": "test",
            "service_type": "ping",
            "host_groups": ["test"],
            "cron_schedule": "@hourly"
        }"#;
        let value: Value = serde_json::from_str(config).expect("Failed to parse config");
        let service = Service::try_from(&value).expect("Failed to parse service");
        assert_eq!(service.name, Some("test".to_string()));
        assert_eq!(service.service_type, ServiceType::Ping);
        assert_eq!(service.host_groups, vec!["test".to_string()]);
        assert_eq!(
            service.cron_schedule.pattern.to_string(),
            Cron::new("@hourly").parse().unwrap().pattern.to_string()
        );
    }
}
