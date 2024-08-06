pub mod cli;
pub mod http;
pub mod kubernetes;
pub mod ping;
pub mod ssh;

use crate::check_loop::CheckResult;
use crate::db::entities;
use crate::prelude::*;
use std::fmt::{self, Debug, Display, Formatter};

use sea_orm::{sea_query, DeriveActiveEnum, EnumIter, Iden};
use serde::de::DeserializeOwned;

use crate::errors::Error;

#[derive(
    Deserialize,
    Debug,
    Serialize,
    Default,
    PartialEq,
    Eq,
    Copy,
    Clone,
    DeriveActiveEnum,
    EnumIter,
    Iden,
)]
#[serde(rename_all = "lowercase")]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(16))")]
pub enum ServiceStatus {
    #[sea_orm(string_value = "ok")]
    Ok,
    #[sea_orm(string_value = "pending")]
    #[default]
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
        write!(f, "{:?}", self)
    }
}

impl ServiceStatus {
    pub fn log(self, msg: &str) {
        match self {
            ServiceStatus::Ok | ServiceStatus::Checking => info!("{}", msg),
            ServiceStatus::Disabled
            | ServiceStatus::Unknown
            | ServiceStatus::Urgent
            | ServiceStatus::Pending
            | ServiceStatus::Warning => warn!("{}", msg),
            ServiceStatus::Critical | ServiceStatus::Error => error!("{}", msg),
        }
    }
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

#[derive(Debug, Deserialize, Serialize)]
pub struct Service {
    #[serde(default = "uuid::Uuid::new_v4")]
    pub id: Uuid,
    /// This is pulled from the config file's key
    pub name: Option<String>,
    pub description: Option<String>,
    pub host_groups: Vec<String>,
    #[serde(alias = "type")]
    pub type_: ServiceType,
    #[serde(
        deserialize_with = "crate::serde::deserialize_croner_cron",
        serialize_with = "crate::serde::serialize_croner_cron"
    )]
    pub cron_schedule: Cron,

    /// Catch-all for the other fields in the config
    #[serde(flatten)]
    pub extra_config: HashMap<String, Value>,

    #[serde(skip)]
    pub config: Option<Box<dyn ServiceTrait>>,
}

impl Service {
    pub fn parse_config(self) -> Result<Self, Error> {
        let value = serde_json::to_value(&self)?;

        if value.is_null() {
            return Ok(self);
        }

        let config = match self.type_ {
            ServiceType::Cli => {
                let value = match cli::CliService::from_config(&value) {
                    Ok(value) => value,
                    Err(e) => {
                        error!("Failed to parse cli service {:?}: {:?}", value, e);
                        return Err(e);
                    }
                };
                Box::new(value) as Box<dyn ServiceTrait>
            }
            ServiceType::Ssh => {
                let value = match ssh::SshService::from_config(&value) {
                    Ok(value) => value,
                    Err(e) => {
                        error!("Failed to parse ssh service {:?}: {:?}", value, e);
                        return Err(e);
                    }
                };
                Box::new(value) as Box<dyn ServiceTrait>
            }
            ServiceType::Ping => {
                let value = match ping::PingService::from_config(&value) {
                    Ok(value) => value,
                    Err(e) => {
                        error!("Failed to parse ping service {:?}: {:?}", value, e);
                        return Err(e);
                    }
                };
                Box::new(value) as Box<dyn ServiceTrait>
            }
            ServiceType::Http => {
                let value = match http::HttpService::from_config(&value) {
                    Ok(value) => value,
                    Err(e) => {
                        error!("Failed to parse http service {:?}: {:?}", value, e);
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
            type_: value.type_.clone(),
            cron_schedule: Cron::new(&value.cron_schedule).parse()?,
            extra_config,
            config: None,
        };
        service = service.parse_config()?;

        Ok(service)
    }
}

#[derive(Deserialize, Debug, Serialize, PartialEq, Eq, Clone, DeriveActiveEnum, EnumIter, Iden)]
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

impl TryFrom<&str> for ServiceType {
    fn try_from(s: &str) -> Result<Self, String> {
        match s {
            "cli" => Ok(ServiceType::Cli),
            _ => Err(format!("Unknown service type: {}", s)),
        }
    }
    type Error = String;
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    async fn test_service_from_model() {
        println!("TODO: this")
    }
}
