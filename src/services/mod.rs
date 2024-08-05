pub mod check;
pub mod cli;
pub mod kubernetes;
pub mod ping;
pub mod ssh;

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
}

#[async_trait]
pub trait ServiceTrait: Debug + Sync + Send {
    async fn run(&self, host: &entities::host::Model) -> Result<ServiceStatus, Error>;

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
    // pub name: String,
    pub description: Option<String>,
    pub host_groups: Vec<String>,
    #[serde(rename = "type")]
    pub type_: ServiceType,
    #[serde(
        deserialize_with = "crate::serde::deserialize_croner_cron",
        serialize_with = "crate::serde::serialize_croner_cron"
    )]
    pub cron_schedule: Cron,
    #[serde(skip)]
    pub config: Option<Box<dyn ServiceTrait>>,
}

impl Service {}

impl TryFrom<&Value> for Service {
    type Error = Error;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        let mut res: Service = serde_json::from_value(value.clone())?;
        let service_config = match res.type_ {
            ServiceType::Cli => {
                let value = cli::CliService::from_config(value)?;
                Box::new(value) as Box<dyn ServiceTrait>
            }
            ServiceType::Ssh => {
                let value = ssh::SshService::from_config(value)?;
                Box::new(value) as Box<dyn ServiceTrait>
            }
        };
        res.config = Some(service_config);
        Ok(res)
    }
}

impl TryFrom<&entities::service::Model> for Service {
    type Error = Error;

    fn try_from(value: &entities::service::Model) -> Result<Self, Self::Error> {
        let host_groups: Vec<String> = serde_json::from_value(value.host_groups.clone())?;

        let mut service = Service {
            id: value.id,
            description: value.description.clone(),
            host_groups,
            type_: value.type_.clone(),
            cron_schedule: value.cron_schedule.parse()?,
            config: None,
        };
        if let Some(config) = &value.config {
            let service_config = match value.type_ {
                ServiceType::Cli => {
                    let value = cli::CliService::from_config(config)?;
                    Box::new(value) as Box<dyn ServiceTrait>
                }
                ServiceType::Ssh => {
                    let value = ssh::SshService::from_config(config)?;
                    Box::new(value) as Box<dyn ServiceTrait>
                }
            };
            service.config = Some(service_config);
        }

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
