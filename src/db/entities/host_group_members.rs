use sea_orm::Set;

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

impl Entity {
    pub async fn upsert(
        db: &DatabaseConnection,
        host_id: &Uuid,
        group_id: &Uuid,
    ) -> Result<Model, Error> {
        let existing = match Entity::find()
            .filter(Column::HostId.eq(*host_id))
            .filter(Column::GroupId.eq(*group_id))
            .one(db)
            .await
        {
            Ok(val) => val,
            Err(DbErr::RecordNotFound(_)) => None,
            Err(err) => return Err(err.into()),
        };

        if let Some(model) = existing {
            return Ok(model);
        }

        debug!(
            "Adding host_group_member for host {} and group {}",
            host_id, group_id
        );
        let model = ActiveModel {
            id: Set(Uuid::new_v4()),
            host_id: Set(*host_id),
            group_id: Set(*group_id),
        };

        let model = model.insert(db).await?;
        Ok(model)
    }
}

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

        for (host_name, host) in config.hosts.clone() {
            let db_host = match super::host::find_by_name(&host_name, db.as_ref()).await? {
                Some(host) => host,
                None => {
                    error!("Host {} not found", host_name);
                    continue;
                }
            };
            for group_name in host.host_groups.clone() {
                // try and get the group otherwise create it
                if let Some((_group, host_list)) = inverted_group_list.get_mut(&group_name) {
                    host_list.push(db_host.id);
                } else {
                    let group = match super::host_group::Entity::find()
                        .filter(super::host_group::Column::Name.eq(&group_name))
                        .one(db.as_ref())
                        .await
                    {
                        Ok(val) => val,
                        Err(err) => {
                            if let DbErr::RecordNotFound(_) = err {
                                None
                            } else {
                                error!("Oh no");
                                return Err(err.into());
                            }
                        }
                    };

                    match group {
                        None => {
                            error!("Couldn't find group {}", group_name);
                            return Err(Error::SqlError(DbErr::RecordNotFound(format!(
                                "Group {} not found",
                                group_name
                            ))));
                        }
                        Some(group) => {
                            inverted_group_list.insert(group_name, (group, vec![db_host.id]));
                        }
                    }
                }
            }
        }

        // make sure the links are there between host group and hosts
        for (group_name, (group, host_ids)) in inverted_group_list {
            debug!("Ensuring links between group {} and hosts", group_name);
            for host_id in host_ids {
                Entity::upsert(db.as_ref(), &host_id, &group.id).await?;
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

        // have to include this because otherwise the members won't exist :)
        super::super::host::Model::update_db_from_config(db.clone(), &configuration)
            .await
            .expect("Failed to update hosts from config");

        // have to include this because otherwise the members won't exist :)
        super::super::host_group::Model::update_db_from_config(db.clone(), &configuration)
            .await
            .expect("Failed to update host groups from config");

        super::Model::update_db_from_config(db.clone(), &configuration)
            .await
            .expect("Failed to load config");

        let host_group_members = super::Entity::find().all(db.as_ref()).await.unwrap();
        assert_eq!(host_group_members.len(), 1);
    }
}
