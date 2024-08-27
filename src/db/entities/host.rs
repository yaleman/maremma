use crate::prelude::*;
use entities::host_group_members::HostToGroups;
use sea_orm::entity::prelude::*;
use sea_orm::IntoActiveModel;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "host")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub name: String,
    pub hostname: String,
    pub check: crate::host::HostCheck,
    pub config: Json,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::host_group::Entity")]
    HostGroup,
}

impl Related<super::service::Entity> for Entity {
    fn to() -> RelationDef {
        super::service_check::Relation::Service.def()
    }

    fn via() -> Option<RelationDef> {
        Some(super::service_check::Relation::Service.def().rev())
    }
}

impl Related<super::service_check::Entity> for Entity {
    fn to() -> RelationDef {
        super::service_check::Relation::Host.def()
    }
}

impl Related<super::host_group::Entity> for Entity {
    fn to() -> RelationDef {
        super::host_group::Relation::Host.def().rev()
    }

    fn via() -> Option<RelationDef> {
        Some(super::host_group::Relation::Host.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    /// Validate that the service checks for this host should still exist
    ///
    /// Used to clean up old service checks that are no longer needed when the host is removed from a group etc
    ///
    pub async fn prune_service_checks(&self, db: &DatabaseConnection) -> Result<(), Error> {
        let result = Entity::find_by_id(self.id)
            .find_with_linked(HostToGroups)
            .all(db)
            .await?;

        debug!("{:#?}", result);

        let (_host, host_groups) = match result.into_iter().next() {
            Some(val) => val,
            None => {
                error!("Failed to find host {}", self.id);
                return Err(Error::HostNotFound(self.id));
            }
        };

        for host_group in host_groups {
            debug!("Host group: {:?}", host_group);

            let _services = host_group
                .find_linked(super::service_group_link::GroupToServices)
                .all(db)
                .await?;
        }
        // let service_checks = super::service_check::Entity::find()
        //     .filter(super::service_check::Column::HostId.eq(self.id))
        //     .all(db)
        //     .await
        //     .inspect_err(|err| {
        //         error!(
        //             "Failed to find service checks for host {} {} {}",
        //             self.id, self.hostname, err,
        //         )
        //     })?;

        Ok(())
    }
}

#[async_trait]
impl MaremmaEntity for Model {
    async fn find_by_name(name: &str, db: &DatabaseConnection) -> Result<Option<Model>, Error> {
        match Entity::find().filter(Column::Name.eq(name)).one(db).await {
            Ok(val) => Ok(val.into_iter().next()),
            Err(err) => {
                error!("Query failed while looking up host '{}': {:?}", name, err);
                Err(err.into())
            }
        }
    }
    async fn update_db_from_config(
        db: &DatabaseConnection,
        config: Arc<Configuration>,
    ) -> Result<(), Error> {
        for (name, host) in config.hosts.clone().into_iter() {
            let model = match Model::find_by_name(&name, db).await {
                Ok(val) => val,
                Err(err) => {
                    error!("Failed to find host '{}': {:?}", name, err);
                    return Err(err);
                }
            };

            match model {
                Some(val) => {
                    debug!("Found host '{:?}'", name);
                    let hostname = match host.hostname {
                        None => name.to_owned(),
                        Some(val) => val,
                    };

                    let mut existing_host = val.into_active_model();

                    existing_host.check.set_if_not_equals(host.check);
                    existing_host
                        .hostname
                        .set_if_not_equals(hostname.to_owned());
                    existing_host.name.set_if_not_equals(name);

                    if existing_host.is_changed() {
                        warn!("Updating {:?}", &existing_host);
                        existing_host.save(db).await?;
                    } else {
                        debug!("No changes to {:?}", &existing_host);
                    }
                }
                None => {
                    let new_host = Model {
                        id: host.id.unwrap_or(Uuid::new_v4()),
                        name: name.to_owned(),
                        hostname: host.hostname.clone().unwrap_or(name.to_string()),
                        check: host.check.clone(),
                        config: json!(host.config.clone()),
                    }
                    .into_active_model();
                    warn!("Creating Host {:?}", new_host.insert(db).await?);
                }
            };
        }
        Ok(())
    }
}

// #[cfg(test)]
pub fn test_host() -> Model {
    Model {
        id: Uuid::new_v4(),
        name: "test_host_name".to_string(),
        hostname: "test_host_hostname".to_string(),
        check: crate::host::HostCheck::Ping,
        config: json!({}),
    }
}

#[cfg(test)]
mod tests {

    use sea_orm::IntoActiveModel;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use tracing::{debug, info};

    use crate::db::entities::MaremmaEntity;
    use crate::db::tests::test_setup;

    #[tokio::test]
    async fn test_host_entity() {
        let (db, _config) = test_setup().await.expect("Failed to start test harness");

        let host = super::test_host();
        info!("saving host...");
        let am = host.clone().into_active_model();
        super::Entity::insert(am).exec(db.as_ref()).await.unwrap();

        let new_host = super::Entity::find()
            .filter(super::Column::Id.eq(host.id))
            .one(db.as_ref())
            .await
            .unwrap()
            .unwrap();
        info!("found it: {:?}", new_host);

        super::Entity::delete_by_id(new_host.id)
            .exec(db.as_ref())
            .await
            .unwrap();

        assert!(super::Entity::find()
            .filter(super::Column::Id.eq(new_host.id))
            .one(db.as_ref())
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_update_db_from_config() {
        let (db, config) = test_setup().await.expect("Failed to start test harness");
        super::Model::update_db_from_config(&db, config)
            .await
            .expect("Failed to load config");
    }
    #[tokio::test]
    async fn test_create_then_search() {
        let (db, _config) = test_setup().await.expect("Failed to start test harness");

        let inserted_host = super::Entity::insert(super::test_host().into_active_model())
            .exec_with_returning(db.as_ref())
            .await
            .expect("Failed to insert host");

        let found_host = super::Model::find_by_name(&super::test_host().name, db.as_ref())
            .await
            .expect("Failed to query host");

        assert!(found_host.is_some());

        assert_eq!(found_host.unwrap().name, inserted_host.name);
    }

    #[tokio::test]
    async fn test_prune_service_checks() {
        let (db, _config) = test_setup().await.expect("Failed to start test harness");

        let host = super::Entity::find()
            .one(db.as_ref())
            .await
            .expect("Failed to run wquery")
            .expect("Failed to find a host?");

        debug!("{:?}", host);

        host.prune_service_checks(db.as_ref())
            .await
            .expect("Failed to prune service checks");
    }
}
