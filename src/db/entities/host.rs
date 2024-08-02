use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "host")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub check: crate::host::HostCheck,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::host_group::Entity")]
    HostGroup,
    #[sea_orm(has_many = "super::service_check::Entity")]
    ServiceCheck,
}

impl Related<super::host_group::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::HostGroup.def()
    }
}

impl Related<super::service_check::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ServiceCheck.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

#[cfg(test)]
pub fn test_host() -> Model {
    Model {
        id: "test_host_id".to_string(),
        name: "test_host_name".to_string(),
        hostname: "test_host_hostname".to_string(),
        check: crate::host::HostCheck::Ping,
    }
}
