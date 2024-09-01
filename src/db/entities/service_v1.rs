use sea_orm::entity::prelude::*;

use crate::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "service")]
/// V1 Service Model, used for the initial version of the service table before `m20240825_create_service_group_link_table`` was done.
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, name = "id")]
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// A list of host group names
    pub host_groups: serde_json::Value,
    pub service_type: ServiceType,
    pub cron_schedule: String,
    #[serde(flatten)]
    pub extra_config: Option<serde_json::Value>,
}

#[derive(Copy, Clone, Debug, EnumIter, sea_orm::DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
