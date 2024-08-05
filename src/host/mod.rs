use sea_orm::entity::prelude::*;
use sea_orm::sea_query;
use std::fmt::Display;

use crate::prelude::*;

pub mod fakehost;
pub mod kube;
pub mod ssh;

#[derive(Deserialize, Serialize, Debug, Clone)]

pub struct Host {
    #[serde(skip, skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,

    #[serde(default = "Default::default")]
    pub check: HostCheck,

    #[serde(default = "Default::default")]
    pub hostname: Option<String>,

    #[serde(default = "Default::default")]
    pub host_groups: Vec<String>,

    // Capture all the other config fields
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Host {
    #[must_use]
    pub fn new(hostname: String, check: HostCheck) -> Self {
        let id = Uuid::new_v4();
        debug!("Creating host: with id: {}", id.hyphenated());
        Self {
            hostname: Some(hostname),
            check,
            host_groups: vec![],
            id: Some(id),
            extra: HashMap::new(),
        }
    }

    pub fn with_hostname(self, hostname: &str) -> Self {
        Self {
            hostname: Some(hostname.to_string()),
            ..self
        }
    }

    pub fn with_host_groups(self, host_groups: Vec<String>) -> Self {
        Self {
            host_groups,
            ..self
        }
    }

    pub fn generate_host_id(name: &str, check: &HostCheck) -> String {
        sha256::digest(&format!("{}:{:?}", name, check))
    }
}

impl From<crate::db::entities::host::Model> for Host {
    fn from(model: crate::db::entities::host::Model) -> Self {
        Self {
            check: model.check,
            hostname: Some(model.hostname),
            host_groups: vec![],
            id: Some(model.id),
            extra: HashMap::new(),
        }
    }
}

#[derive(
    Deserialize, Debug, Serialize, Default, PartialEq, Eq, Clone, DeriveActiveEnum, EnumIter, Iden,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(1))")]
#[serde(rename_all = "lowercase")]
pub enum HostCheck {
    /// No checks done
    #[sea_orm(string_value = "n")]
    None,
    /// Checks by pinging the host
    #[default]
    #[sea_orm(string_value = "p")]
    Ping,
    /// Checks by trying to SSH to the host
    #[sea_orm(string_value = "s")]
    SshHost,
    /// Checks we can connect to the Kubernetes API
    #[sea_orm(string_value = "k")]
    KubeHost,
}

impl Display for HostCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostCheck::None => write!(f, "None"),
            HostCheck::Ping => write!(f, "Ping"),
            HostCheck::SshHost => write!(f, "SSH"),
            HostCheck::KubeHost => {
                write!(f, "Kubernetes")
            }
        }
    }
}

#[async_trait]
pub trait GenericHost
where
    Self: std::marker::Sized,
{
    async fn check_up(&self) -> Result<bool, crate::errors::Error>;

    fn name(&self) -> String;

    fn id(&self) -> String;

    fn try_from_config(config: serde_json::Value) -> Result<Self, Error>
    where
        Self: Sized;
}
