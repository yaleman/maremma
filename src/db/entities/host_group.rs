use crate::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "host_group")]
/// Host group model
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(database_type = "String", unique, indexed)]
    pub name: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::host::Entity")]
    Host,
    #[sea_orm(has_many = "super::service::Entity")]
    Service,
    #[sea_orm(has_many = "super::service::Entity")]
    ServiceGroupLink,
}

#[cfg(not(tarpaulin_include))]
impl Related<super::host::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Host.def()
    }

    fn via() -> Option<RelationDef> {
        Some(super::host::Relation::HostGroup.def().rev())
    }
}

#[cfg(not(tarpaulin_include))]
impl Related<super::service::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Service.def()
    }

    fn via() -> Option<RelationDef> {
        Some(super::service::Relation::HostGroup.def().rev())
    }
}

#[cfg(not(tarpaulin_include))]
impl Related<super::host_group_members::Entity> for Entity {
    fn to() -> RelationDef {
        super::host_group_members::Relation::HostGroup.def()
    }
}

#[cfg(not(tarpaulin_include))]
impl Related<super::service_group_link::Entity> for Entity {
    fn to() -> RelationDef {
        super::service_group_link::Relation::HostGroup.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

#[async_trait]
impl MaremmaEntity for Model {
    async fn find_by_name(name: &str, db: &DatabaseConnection) -> Result<Option<Model>, Error> {
        Entity::find()
            .filter(Column::Name.eq(name))
            .one(db)
            .await
            .map_err(Error::from)
    }

    async fn update_db_from_config(
        db: &DatabaseConnection,
        config: SendableConfig,
    ) -> Result<(), Error> {
        let mut known_group_list: Vec<String> = Entity::find()
            .all(db)
            .await?
            .into_iter()
            .map(|x| x.name)
            .collect();

        // add the group names to the known group list
        for (_host_name, host) in config.read().await.hosts.iter() {
            for group_name in &host.host_groups {
                // if we already have the group name we don't need to add it to the db
                if known_group_list.contains(group_name) {
                    debug!("already have {}", group_name);
                    continue;
                }

                // we haven't added it to the list, so we're going to have to see if it's already in the database.
                if Model::find_by_name(group_name, db).await?.is_none() {
                    Entity::insert(
                        Model {
                            id: Uuid::new_v4(),
                            name: group_name.to_owned(),
                        }
                        .into_active_model(),
                    )
                    .exec(db)
                    .await?;
                } else {
                    debug!("already have {}", group_name);
                }

                known_group_list.push(group_name.to_string());
            }
        }

        for (service_name, service) in &config.read().await.services {
            for group_name in service.host_groups.iter() {
                if known_group_list.contains(group_name) {
                    continue;
                }
                if Model::find_by_name(group_name, db).await?.is_none() {
                    debug!("Adding host group {}", group_name);
                    Entity::insert(
                        Model {
                            id: Uuid::new_v4(),
                            name: group_name.to_owned(),
                        }
                        .into_active_model(),
                    )
                    .exec_with_returning(db)
                    .await?;
                    info!(
                        "Added group {:?} from service {:?} to DB",
                        &service_name, group_name
                    );
                } else {
                    debug!("Already have group {}", group_name);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use uuid::Uuid;

    use crate::config::Configuration;
    use crate::db::tests::test_setup;
    use crate::prelude::MaremmaEntity;

    #[tokio::test]
    async fn test_update_db_from_config() {
        let (db, config) = test_setup().await.expect("Failed to start test harness");

        super::Model::update_db_from_config(&db, config)
            .await
            .expect("Failed to load config");
    }
    #[tokio::test]
    async fn test_failing_update_db_from_config_hg() {
        use sea_orm::{DatabaseBackend, MockDatabase};

        let db = MockDatabase::new(DatabaseBackend::Sqlite)
            .append_query_results([[super::Model {
                id: Uuid::new_v4(),
                name: "Test".to_owned(),
            }]])
            .into_connection();

        let res =
            super::Model::update_db_from_config(&db, Configuration::load_test_config().await).await;

        dbg!(&res);
        assert!(res.is_err());
    }
}
