//! Links services to groups

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
        todo!()
    }

    async fn update_db_from_config(
        db: &DatabaseConnection,
        config: Arc<Configuration>,
    ) -> Result<(), Error> {
        let services = entities::service::Entity::find().all(db).await?;

        for group_name in config.groups() {
            if let Some(group) = entities::host_group::Model::find_by_name(&group_name, db).await? {
                for service in &services {
                    if let Some(host_groups) = service.host_groups.as_array() {
                        if host_groups.contains(&json!(group.name)) {
                            debug!("Checking service={} in group={}", service.name, group.name);
                            if entities::service_group_link::Entity::find()
                                .filter(
                                    Column::ServiceId
                                        .eq(service.id)
                                        .and(Column::GroupId.eq(group.id)),
                                )
                                .one(db)
                                .await?
                                .is_none()
                            {
                                debug!(
                                    "Adding link for service={} => group={}",
                                    service.name, group.name
                                );
                                entities::service_group_link::ActiveModel {
                                    id: Set(Uuid::new_v4()),
                                    service_id: Set(service.id),
                                    group_id: Set(group.id),
                                }
                                .insert(db)
                                .await?;
                            }
                        }
                    }
                }
            };
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

        super::super::service::Model::update_db_from_config(&db, config.clone())
            .await
            .expect("Failed to update services from config");

        super::Model::update_db_from_config(&db, config)
            .await
            .expect("Failed to load config");

        let (service, groups) = entities::service::Entity::find()
            .filter(entities::service::Column::HostGroups.ne(Json::Null))
            .find_with_linked(entities::service_group_link::ServiceToGroups)
            .all(db.as_ref())
            .await
            .expect("Failed to run query looking for a service with host groups")
            .into_iter()
            .next()
            .expect("Uh...");

        dbg!(service);
        dbg!(groups);
    }
}
