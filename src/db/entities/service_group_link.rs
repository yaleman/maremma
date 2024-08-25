//! Links services to groups

use crate::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "service_group_link")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub service_id: Uuid,
    pub group_id: Uuid,
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    Service,
    HostGroup,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::Service => Entity::belongs_to(super::service::Entity)
                .from(Column::ServiceId)
                .to(super::service::Column::Id)
                .into(),
            Self::HostGroup => Entity::belongs_to(super::host_group::Entity)
                .from(Column::GroupId)
                .to(super::host_group::Column::Id)
                .into(),
        }
    }
}

// This lets you find related groups for a service
pub struct ServiceToGroups;

impl Linked for ServiceToGroups {
    type FromEntity = entities::service::Entity;
    type ToEntity = entities::host_group::Entity;

    fn link(&self) -> Vec<RelationDef> {
        vec![
            Relation::Service.def().rev(),
            Entity::belongs_to(super::host_group::Entity)
                .from(Column::GroupId)
                .to(super::host_group::Column::Id)
                .into(),
        ]
    }
}

// This lets you find related services for a group
pub struct GroupToServices;

impl Linked for GroupToServices {
    type FromEntity = super::host_group::Entity;
    type ToEntity = super::service::Entity;

    fn link(&self) -> Vec<RelationDef> {
        vec![
            Relation::HostGroup.def().rev(),
            Entity::belongs_to(super::service::Entity)
                .from(Column::ServiceId)
                .to(super::service::Column::Id)
                .into(),
        ]
    }
}

impl ActiveModelBehavior for ActiveModel {}

#[async_trait]
impl MaremmaEntity for Model {
    async fn find_by_name(_name: &str, _db: &DatabaseConnection) -> Result<Option<Model>, Error> {
        todo!()
    }

    async fn update_db_from_config(
        _db: &DatabaseConnection,
        _config: Arc<Configuration>,
    ) -> Result<(), Error> {
        // todo!();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::db::tests::test_setup;
    use crate::prelude::*;

    #[tokio::test]
    async fn test_update_db_from_config() {
        let (db, config) = test_setup().await.expect("Failed to start test harness");

        // have to include this because otherwise the members won't exist :)
        super::super::host::Model::update_db_from_config(&db, config.clone())
            .await
            .expect("Failed to update hosts from config");

        // have to include this because otherwise the members won't exist :)
        super::super::host_group::Model::update_db_from_config(&db, config.clone())
            .await
            .expect("Failed to update host groups from config");

        super::Model::update_db_from_config(&db, config)
            .await
            .expect("Failed to load config");

        let host_group_members = super::Entity::find().all(db.as_ref()).await.unwrap();
        assert_ne!(host_group_members.len(), 1);
    }
}
