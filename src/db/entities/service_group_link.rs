//! Links services to groups

use entities::{host_group, service};
use sea_orm::Set;

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
        Err(Error::NotImplemented)
    }

    async fn update_db_from_config(
        db: &DatabaseConnection,
        config: Arc<RwLock<Configuration>>,
    ) -> Result<(), Error> {
        for (service_name, service) in &config.read().await.services {
            let service_model = service::Model::find_by_name(service_name, db)
                .await?
                .ok_or(Error::ServiceNotFoundByName(service_name.to_string()))?;

            for group_name in service.host_groups.iter() {
                debug!("Service: {} Group: {}", service_name, group_name);

                let group_model = host_group::Model::find_by_name(group_name, db)
                    .await?
                    .ok_or(Error::HostGroupNotFoundByName(group_name.to_string()))?;

                if Entity::find()
                    .filter(
                        Column::ServiceId
                            .eq(service_model.id)
                            .and(Column::GroupId.eq(group_model.id)),
                    )
                    .one(db)
                    .await?
                    .is_none()
                {
                    debug!(
                        "Need to create link for Service: {} Group: {}",
                        service_name, group_name
                    );
                    ActiveModel {
                        id: Set(Uuid::new_v4()),
                        service_id: Set(service_model.id),
                        group_id: Set(group_model.id),
                    }
                    .insert(db)
                    .await?;
                };
            }
        }

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

        super::super::host_group::Model::update_db_from_config(&db, config.clone())
            .await
            .expect("Failed to update services from config");
        super::super::service::Model::update_db_from_config(&db, config.clone())
            .await
            .expect("Failed to update services from config");

        super::Model::update_db_from_config(&db, config)
            .await
            .expect("Failed to load config");
    }

    #[tokio::test]
    async fn test_find_by_name() {
        // this should error
        let (db, _config) = test_setup().await.expect("Failed to start test harness");

        let res = super::Model::find_by_name("test", &db).await;

        assert!(res.is_err());
        assert_eq!(res.err().unwrap(), Error::NotImplemented);
    }
}
