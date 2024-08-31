use crate::prelude::*;
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

#[cfg(not(tarpaulin_include))]
impl Related<super::service::Entity> for Entity {
    fn to() -> RelationDef {
        super::service_check::Relation::Service.def()
    }

    fn via() -> Option<RelationDef> {
        Some(super::service_check::Relation::Service.def().rev())
    }
}

#[cfg(not(tarpaulin_include))]
impl Related<super::service_check::Entity> for Entity {
    fn to() -> RelationDef {
        super::service_check::Relation::Host.def()
    }
}

#[cfg(not(tarpaulin_include))]
impl Related<super::host_group::Entity> for Entity {
    fn to() -> RelationDef {
        super::host_group::Relation::Host.def().rev()
    }

    fn via() -> Option<RelationDef> {
        Some(super::host_group::Relation::Host.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}

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
        config: SendableConfig,
    ) -> Result<(), Error> {
        for (name, host) in &config.read().await.hosts {
            let model = match Model::find_by_name(name, db).await {
                Ok(val) => val,
                Err(err) => {
                    error!("Failed to find host '{}': {:?}", name, err);
                    return Err(err);
                }
            };

            match model {
                Some(val) => {
                    debug!("Found host '{:?}'", name);
                    let hostname = match &host.hostname {
                        None => name,
                        Some(val) => val,
                    };

                    let mut existing_host = val.into_active_model();

                    existing_host.check.set_if_not_equals(host.check.to_owned());
                    existing_host
                        .hostname
                        .set_if_not_equals(hostname.to_owned());
                    existing_host.name.set_if_not_equals(name.to_string());
                    existing_host.config.set_if_not_equals(json!(host.config));

                    if existing_host.is_changed() {
                        info!("Updating {:?}", &existing_host);
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
                    info!("Creating Host {:?}", new_host.insert(db).await?);
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
    use tracing::info;
    use uuid::Uuid;

    use crate::config::Configuration;
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
    async fn test_failing_update_db_from_config_host() {
        use sea_orm::{DatabaseBackend, MockDatabase};

        let db = MockDatabase::new(DatabaseBackend::Sqlite)
            .append_query_results([[super::Model {
                id: Uuid::new_v4(),
                name: "foo".to_string(),
                hostname: "foo.example.com".to_owned(),
                check: crate::host::HostCheck::None,
                config: serde_json::json!({}),
            }]])
            .into_connection();

        let res =
            super::Model::update_db_from_config(&db, Configuration::load_test_config().await).await;

        dbg!(&res);
        assert!(res.is_err());
    }
}
