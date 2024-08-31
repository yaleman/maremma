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
                .from(Column::HostId)
                .to(super::host::Column::Id)
                .into(),
            Self::HostGroup => Entity::belongs_to(super::host_group::Entity)
                .from(Column::GroupId)
                .to(super::host_group::Column::Id)
                .into(),
        }
    }
}

// This lets you find related groups for a host
pub struct HostToGroups;

impl Linked for HostToGroups {
    type FromEntity = entities::host::Entity;
    type ToEntity = entities::host_group::Entity;

    fn link(&self) -> Vec<RelationDef> {
        vec![
            Relation::Host.def().rev(),
            Entity::belongs_to(super::host_group::Entity)
                .from(Column::GroupId)
                .to(super::host_group::Column::Id)
                .into(),
        ]
    }
}

// This lets you find related hosts for a group
pub struct GroupToHosts;

impl Linked for GroupToHosts {
    type FromEntity = super::host_group::Entity;
    type ToEntity = super::host::Entity;

    fn link(&self) -> Vec<RelationDef> {
        vec![
            Relation::HostGroup.def().rev(),
            Entity::belongs_to(super::host::Entity)
                .from(Column::HostId)
                .to(super::host::Column::Id)
                .into(),
        ]
    }
}

impl ActiveModelBehavior for ActiveModel {}

impl Entity {
    pub async fn upsert(
        db: &DatabaseConnection,
        host_id: &Uuid,
        group_id: &Uuid,
    ) -> Result<Model, Error> {
        let existing = Entity::find()
            .filter(Column::HostId.eq(*host_id))
            .filter(Column::GroupId.eq(*group_id))
            .one(db)
            .await?;
        match existing {
            Some(val) => Ok(val),
            None => {
                debug!(
                    "Adding host_group_member for host {} and group {}",
                    host_id, group_id
                );
                ActiveModel {
                    id: Set(Uuid::new_v4()),
                    host_id: Set(*host_id),
                    group_id: Set(*group_id),
                }
                .insert(db)
                .await
                .map_err(Error::from)
            }
        }
    }
}

#[async_trait]
impl MaremmaEntity for Model {
    async fn find_by_name(_name: &str, _db: &DatabaseConnection) -> Result<Option<Model>, Error> {
        Err(Error::NotImplemented)
    }

    async fn update_db_from_config(
        db: &DatabaseConnection,
        config: SendableConfig,
    ) -> Result<(), Error> {
        // group -> (group def, host ids)
        let mut inverted_group_list: HashMap<String, (super::host_group::Model, Vec<Uuid>)> =
            HashMap::new();

        for (host_name, host) in &config.read().await.hosts {
            let db_host = match super::host::Model::find_by_name(host_name, db).await? {
                Some(host) => host,
                None => {
                    error!(
                        "Host '{}' not found while updating host group members!",
                        host_name
                    );
                    continue;
                }
            };
            for group_name in &host.host_groups {
                // try and get the group otherwise create it
                if let Some((_group, host_list)) = inverted_group_list.get_mut(group_name) {
                    host_list.push(db_host.id);
                } else {
                    let group = super::host_group::Entity::find()
                        .filter(super::host_group::Column::Name.eq(group_name))
                        .one(db)
                        .await?;

                    match group {
                        None => {
                            return Err(Error::HostGroupNotFoundByName(group_name.clone()));
                        }
                        Some(group) => {
                            inverted_group_list
                                .insert(group_name.clone(), (group, vec![db_host.id]));
                        }
                    }
                }
            }
        }

        // make sure the links are there between host group and hosts
        for (group_name, (group, host_ids)) in inverted_group_list {
            debug!("Ensuring links between group {} and hosts", group_name);
            for host_id in host_ids {
                Entity::upsert(db, &host_id, &group.id).await?;
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
    async fn test_find_by_name() {
        // this should error
        let (db, _config) = test_setup().await.expect("Failed to start test harness");

        let res = super::Model::find_by_name("test", &db).await;

        assert!(res.is_err());
        assert_eq!(res.err().unwrap(), Error::NotImplemented);
    }

    #[tokio::test]
    async fn test_failing_update_db_from_config_hgm() {
        use sea_orm::{DatabaseBackend, MockDatabase};

        let db = MockDatabase::new(DatabaseBackend::Sqlite)
            .append_query_results([[super::Model {
                id: Uuid::new_v4(),
                host_id: Uuid::new_v4(),
                group_id: Uuid::new_v4(),
            }]])
            .into_connection();

        let res =
            super::Model::update_db_from_config(&db, Configuration::load_test_config().await).await;

        dbg!(&res);
        assert!(res.is_err());
    }
}
