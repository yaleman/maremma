pub mod cli;
pub mod http;
pub mod kubernetes;
pub mod ping;
pub mod ssh;

use crate::check_loop::CheckResult;
use crate::db::entities;
use crate::prelude::*;
use std::fmt::{self, Debug, Display, Formatter};

use schemars::JsonSchema;
use sea_orm::{sea_query, DeriveActiveEnum, EnumIter, Iden};
use serde::de::DeserializeOwned;

use crate::errors::Error;
#[derive(
    Deserialize, Debug, Serialize, PartialEq, Eq, Copy, Clone, DeriveActiveEnum, EnumIter, Iden,
)]
#[serde(rename_all = "lowercase")]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(16))")]
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
    // Returns the cell background colour for the status, from the [bootstrap colours](https://getbootstrap.com/docs/5.3/utilities/colors/)
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

    // Returns the text colour for the status, from the [bootstrap colours](https://getbootstrap.com/docs/5.3/utilities/colors/)
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
pub trait ServiceTrait: Debug + Sync + Send {
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error>;

    fn from_config(config: &Value) -> Result<Self, Error>
    where
        Self: Sized + DeserializeOwned,
    {
        serde_json::from_value(config.clone()).map_err(Error::from)
    }
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct Service {
    #[serde(default = "uuid::Uuid::new_v4")]
    pub id: Uuid,
    /// This is pulled from the config file's key
    pub name: Option<String>,
    pub description: Option<String>,
    pub host_groups: Vec<String>,
    pub service_type: ServiceType,
    #[serde(
        deserialize_with = "crate::serde::deserialize_croner_cron",
        serialize_with = "crate::serde::serialize_croner_cron"
    )]
    #[schemars(with = "String")]
    /// Cron schedule for the service, eg `@hourly`, `* * * * * *` or `0 0 * * *`
    pub cron_schedule: Cron,

    /// Catch-all for the other fields in the config
    #[serde(flatten)]
    pub extra_config: HashMap<String, Value>,

    #[serde(skip)]
    pub config: Option<Box<dyn ServiceTrait>>,
}

impl Service {
    /// because services are stored in the database as a json field, we need to parse the config and store the type internally
    pub fn parse_config(self) -> Result<Self, Error> {
        let value = serde_json::to_value(&self)?;
        let service_identifier = self
            .name
            .clone()
            .unwrap_or(self.id.hyphenated().to_string());
        let config = match self.service_type {
            ServiceType::Cli => {
                let value = match cli::CliService::from_config(&value) {
                    Ok(value) => value,
                    Err(e) => {
                        error!(
                            "Failed to parse cli service {} {:?}: {:?}",
                            service_identifier, value, e
                        );
                        return Err(e);
                    }
                };
                Box::new(value) as Box<dyn ServiceTrait>
            }
            ServiceType::Ssh => {
                let value = match ssh::SshService::from_config(&value) {
                    Ok(value) => value,
                    Err(e) => {
                        error!(
                            "Failed to parse ssh service {} {:?}: {:?}",
                            service_identifier, value, e
                        );
                        return Err(e);
                    }
                };
                Box::new(value) as Box<dyn ServiceTrait>
            }
            ServiceType::Ping => {
                let value = match ping::PingService::from_config(&value) {
                    Ok(value) => value,
                    Err(e) => {
                        error!(
                            "Failed to parse ping service {} {:?}: {:?}",
                            service_identifier, value, e
                        );
                        return Err(e);
                    }
                };
                Box::new(value) as Box<dyn ServiceTrait>
            }
            ServiceType::Http => {
                let value = match http::HttpService::from_config(&value) {
                    Ok(value) => value,
                    Err(e) => {
                        error!(
                            "Failed to parse http service {} {:?}: {:?}",
                            service_identifier, value, e
                        );
                        return Err(e);
                    }
                };
                Box::new(value) as Box<dyn ServiceTrait>
            }
        };
        Ok(Self {
            config: Some(config),
            ..self
        })
    }
}

impl TryFrom<&Value> for Service {
    type Error = Error;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        let res: Service = serde_json::from_value(value.clone())?;
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

        let mut service = Service {
            id: value.id,
            name: Some(value.name.clone()),
            description: value.description.clone(),
            host_groups,
            service_type: value.service_type.clone(),
            cron_schedule: Cron::new(&value.cron_schedule).parse()?,
            extra_config,
            config: None,
        };
        service = service.parse_config()?;

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
)]
#[serde(rename_all = "lowercase")]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(5))")]
pub enum ServiceType {
    #[sea_orm(string_value = "cli")]
    Cli,
    #[sea_orm(string_value = "ssh")]
    Ssh,
    #[sea_orm(string_value = "ping")]
    Ping,
    #[sea_orm(string_value = "http")]
    Http,
}

impl Display for ServiceType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
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
        let service_from_value: Service = (&service_as_value)
            .try_into()
            .expect("Failed to convert value to service");

        service_from_value
            .parse_config()
            .expect("Failed to parse config");
    }

    #[test]
    fn test_display_service_type() {
        assert_eq!(format!("{}", ServiceType::Cli), "Cli");
        assert_eq!(format!("{}", ServiceType::Ssh), "Ssh");
        assert_eq!(format!("{}", ServiceType::Ping), "Ping");
        assert_eq!(format!("{}", ServiceType::Http), "Http");
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
            "cron_schedule": "@hourly"
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
