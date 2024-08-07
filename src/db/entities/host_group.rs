use crate::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "host_group")]
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
}

impl Related<super::host::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Host.def()
    }

    fn via() -> Option<RelationDef> {
        Some(super::host::Relation::HostGroup.def().rev())
    }
}

impl Related<super::host_group_members::Entity> for Entity {
    fn to() -> RelationDef {
        super::host_group_members::Relation::HostGroup.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

pub async fn find_by_name(name: &str, db: &DatabaseConnection) -> Result<Option<Model>, Error> {
    Entity::find()
        .filter(Column::Name.eq(name))
        .one(db)
        .await
        .map_err(Error::from)
}

#[async_trait]
impl MaremmaEntity for Model {
    async fn update_db_from_config(
        db: Arc<DatabaseConnection>,
        config: &Configuration,
    ) -> Result<(), Error> {
        let mut known_group_list: Vec<String> = Entity::find()
            .all(db.as_ref())
            .await?
            .into_iter()
            .map(|x| x.name)
            .collect();

        // add the group names to the known group list
        for (_host_name, host) in config.hosts.iter() {
            for group_name in host.host_groups.clone() {
                // if we already have the group name we don't need to add it to the db
                if known_group_list.contains(&group_name) {
                    debug!("already have {}", group_name);
                    continue;
                }

                // we haven't added it to the list, so we're going to have to see if it's already in the database.
                if find_by_name(&group_name, db.as_ref()).await?.is_none() {
                    Entity::insert(
                        Model {
                            id: Uuid::new_v4(),
                            name: group_name.to_owned(),
                        }
                        .into_active_model(),
                    )
                    .exec(db.as_ref())
                    .await?;
                } else {
                    debug!("already have {}", group_name);
                }

                known_group_list.push(group_name);
            }
        }

        if let Some(services) = config.services.clone() {
            if let Some(services) = services.as_object().cloned() {
                for (service_name, service) in services {
                    match serde_json::from_value::<Service>(service.clone()) {
                        Ok(service) => {
                            for group_name in service.host_groups.iter() {
                                if known_group_list.contains(group_name) {
                                    continue;
                                }
                                if find_by_name(group_name, db.as_ref()).await?.is_none() {
                                    debug!("Adding host group {}", group_name);
                                    Entity::insert(
                                        Model {
                                            id: Uuid::new_v4(),
                                            name: group_name.to_owned(),
                                        }
                                        .into_active_model(),
                                    )
                                    .exec_with_returning(db.as_ref())
                                    .await?;
                                    warn!(
                                        "Added group {:?} from service {:?} to DB",
                                        &service_name, group_name
                                    );
                                } else {
                                    debug!("Already have group {}", group_name);
                                }
                            }
                        }
                        Err(err) => {
                            error!("Couldn't parse service: {:?} -> {:?}", service, err);
                        }
                    };
                }
            } else {
                warn!("Couldn't parse service map from configuration");
                debug!("Services: {:?}", services);
            }
        } else {
            warn!("No services found in configuration");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::prelude::MaremmaEntity;

    #[tokio::test]
    async fn test_update_db_from_config() {
        let db = Arc::new(
            crate::db::test_connect()
                .await
                .expect("Failed to connect to database"),
        );

        let configuration =
            crate::config::Configuration::new(Some(PathBuf::from("maremma.example.json")))
                .await
                .expect("Failed to load configuration");

        super::Model::update_db_from_config(db, &configuration)
            .await
            .expect("Failed to load config");
    }
}
