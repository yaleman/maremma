//! How the system represents hosts

use schemars::JsonSchema;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query;
use std::fmt::Display;

use crate::prelude::*;

/// Implements "Fakehost" which is used for local checks
pub mod fakehost;
/// Implements the Kubernetes host check
pub mod kube;
/// Implements the SSH-based host check
pub mod ssh;

#[derive(Deserialize, Serialize, Debug, Clone, JsonSchema)]
/// A generic host
pub struct Host {
    #[serde(skip, skip_serializing_if = "Option::is_none")]
    /// Internal ID
    pub id: Option<Uuid>,

    #[serde(default)]
    /// The kind of check
    pub check: HostCheck,

    /// The hostname
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,

    #[serde(default)]
    /// Groups that this host is part of
    pub host_groups: Vec<String>,

    #[serde(default)]
    /// Extra configuration for services, the key matches the service name
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub config: HashMap<String, serde_json::Value>,

    /// Captures all the other config fields, if any
    #[serde(flatten)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Host {
    /// Build a new host
    pub fn new(hostname: String, check: HostCheck) -> Self {
        let id = Uuid::new_v4();
        Self {
            hostname: Some(hostname),
            check,
            host_groups: vec![],
            id: Some(id),
            config: HashMap::new(),
            extra: HashMap::new(),
        }
    }
}

impl From<crate::db::entities::host::Model> for Host {
    fn from(model: crate::db::entities::host::Model) -> Self {
        Self {
            check: model.check,
            hostname: Some(model.hostname),
            host_groups: vec![],
            id: Some(model.id),
            config: HashMap::new(),
            extra: HashMap::new(),
        }
    }
}

#[derive(
    Deserialize,
    Debug,
    Serialize,
    Default,
    PartialEq,
    Eq,
    Clone,
    DeriveActiveEnum,
    EnumIter,
    Iden,
    JsonSchema,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(1))")]
#[serde(rename_all = "lowercase")]
/// The kind of check to perform to ensure the host is up
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
    Ssh,
    /// Checks we can connect to the Kubernetes API
    #[sea_orm(string_value = "k")]
    Kubernetes,
}

impl Display for HostCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostCheck::None => write!(f, "None"),
            HostCheck::Ping => write!(f, "Ping"),
            HostCheck::Ssh => write!(f, "SSH"),
            HostCheck::Kubernetes => {
                write!(f, "Kubernetes")
            }
        }
    }
}

#[async_trait]
/// Host-check type things
pub trait GenericHost
where
    Self: std::marker::Sized,
{
    /// Check if the host is available
    async fn check_up(&self) -> Result<bool, crate::errors::Error>;

    /// Create this from [serde_json::Value]
    fn try_from_config(config: serde_json::Value) -> Result<Self, Error>
    where
        Self: Sized;
}

#[cfg(test)]
mod tests {

    use crate::db::tests::test_setup;
    use crate::host::HostCheck;

    #[tokio::test]
    async fn test_host_from_host() {
        use super::*;

        let (db, _config) = test_setup().await.expect("Failed to setup test");

        let host: Host = entities::host::Entity::find()
            .one(db.as_ref())
            .await
            .expect("Failed to query host")
            .expect("Failed to find test host")
            .into();

        assert!(host.id.is_some());
    }

    #[test]

    fn test_hostcheck_display() {
        for (check, result) in [
            (HostCheck::None, "None"),
            (HostCheck::Ping, "Ping"),
            (HostCheck::Ssh, "SSH"),
            (HostCheck::Kubernetes, "Kubernetes"),
        ] {
            assert_eq!(check.to_string(), result);
        }
    }
}
