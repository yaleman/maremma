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
pub trait GenericHost
where
    Self: std::marker::Sized,
{
    async fn check_up(&self) -> Result<bool, crate::errors::Error>;

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
