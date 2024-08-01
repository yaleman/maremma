pub mod check;
pub mod cli;
pub mod ssh;

use crate::prelude::*;
use std::fmt::{self, Debug, Display, Formatter};

use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;

use crate::errors::Error;

#[derive(Debug, Copy, Clone)]
pub enum ServiceStatus {
    Ok,
    Pending,
    Critical,
    Checking,
    Warning,
    Error,
    Unknown,
    /// Run this as soon as possible
    Urgent,
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
    async fn run(&self, host: &Host) -> Result<ServiceStatus, Error>;

    fn from_config(config: &Value) -> Result<Self, Error>
    where
        Self: Sized + DeserializeOwned,
    {
        serde_json::from_value(config.clone()).map_err(Error::from)
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Service {
    pub name: String,
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

    #[serde(default)]
    pub last_runtime: DateTime<Utc>,
}

impl Service {
    pub fn id(&self) -> String {
        generate_service_id(&self.name, &self.type_)
    }
}

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

pub fn generate_service_id(name: &str, service_type: &ServiceType) -> String {
    sha256::digest(&format!("{}:{}", name, service_type))
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceType {
    Cli,
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
