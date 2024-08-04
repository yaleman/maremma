use crate::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "host_group_members")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub host_id: Uuid,
    pub group_id: Uuid,
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    Host,
    HostGroup,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::Host => Entity::belongs_to(super::host::Entity)
                .from(Column::Id)
                .to(super::host::Column::Id)
                .into(),
            Self::HostGroup => Entity::belongs_to(super::host_group::Entity)
                .from(Column::HostId)
                .to(super::host_group::Column::Id)
                .into(),
        }
    }
}

impl ActiveModelBehavior for ActiveModel {}

#[async_trait]
impl MaremmaEntity for Model {
    async fn update_db_from_config(
        db: Arc<DatabaseConnection>,
        config: &Configuration,
    ) -> Result<(), Error> {
        // group -> (group def, host ids)
        let mut inverted_group_list: HashMap<String, (super::host_group::Model, Vec<Uuid>)> =
            HashMap::new();

        if config.hosts.is_empty() {
            error!("Host list is empty!");
        }

        for (_host_name, host) in config.hosts.clone() {
            for group_name in host.host_groups.clone() {
                // try and get the group otherwise create it
                if let Some(group) = inverted_group_list.get_mut(&group_name) {
                    group.1.push(host.id);
                } else {
                    let group = match super::host_group::Entity::find()
                        .filter(super::host_group::Column::Name.eq(&group_name))
                        .one(db.as_ref())
                        .await
                    {
                        Ok(val) => val,
                        Err(err) => {
                            if let DbErr::RecordNotFound(_) = err {
                                Some(
                                    super::host_group::Entity::insert(
                                        super::host_group::Model {
                                            id: Uuid::new_v4(),
                                            name: group_name.to_owned(),
                                        }
                                        .into_active_model(),
                                    )
                                    .exec_with_returning(db.as_ref())
                                    .await?,
                                )
                            } else {
                                error!("Oh no");
                                return Err(err.into());
                            }
                        }
                    };

                    let group = match group {
                        Some(val) => val,
                        None => {
                            super::host_group::Entity::insert(
                                super::host_group::Model {
                                    id: Uuid::new_v4(),
                                    name: group_name.to_owned(),
                                }
                                .into_active_model(),
                            )
                            .exec_with_returning(db.as_ref())
                            .await?
                        }
                    };
                    inverted_group_list.insert(group_name, (group, vec![host.id]));
                }
            }
        }

        // make sure the links are there between host group and hosts
        for (group_name, (group, host_ids)) in inverted_group_list {
            debug!("Ensuring links between group {} and hosts", group_name);
            for host_id in host_ids {
                let host_group_member = match Entity::find()
                    .filter(Column::HostId.eq(host_id))
                    .filter(Column::GroupId.eq(group.id))
                    .one(db.as_ref())
                    .await
                {
                    Ok(val) => val,
                    Err(DbErr::RecordNotFound(_)) => None,
                    Err(err) => {
                        error!("Oh no");
                        return Err(err.into());
                    }
                };

                if host_group_member.is_none() {
                    Entity::insert(
                        Model {
                            id: Uuid::new_v4(),
                            host_id,
                            group_id: group.id,
                        }
                        .into_active_model(),
                    )
                    .exec(db.as_ref())
                    .await?;
                } else {
                    debug!(
                        "Link between host {} and group {} already exists",
                        host_id, group.id
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{prelude::*, setup_logging};

    #[tokio::test]
    async fn test_update_db_from_config() {
        let _ = setup_logging(true);
        let db = Arc::new(
            crate::db::test_connect()
                .await
                .expect("Failed to connect to database"),
        );

        let configuration = crate::config::Configuration::load_test_config().await;

        super::Model::update_db_from_config(db.clone(), &configuration)
            .await
            .expect("Failed to load config");

        let host_group_members = super::Entity::find().all(db.as_ref()).await.unwrap();
        assert_eq!(host_group_members.len(), 1);
    }
}
