use sea_orm::entity::prelude::*;
use sea_orm::TryIntoModel;

use crate::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "service")]
/// Service database model
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, name = "id")]
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// A list of host group names
    pub service_type: ServiceType,
    pub cron_schedule: String,
    pub extra_config: Json,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::host_group::Entity")]
    HostGroup,

    #[sea_orm(has_many = "super::service_check::Entity")]
    ServiceCheck,
}

impl Related<super::host::Entity> for Entity {
    fn to() -> RelationDef {
        super::service_check::Relation::Host.def().rev()
    }

    fn via() -> Option<RelationDef> {
        Some(super::service_check::Relation::Service.def())
    }
}

impl Related<super::host_group::Entity> for Entity {
    fn to() -> RelationDef {
        super::host_group::Relation::Service.def().rev()
    }

    fn via() -> Option<RelationDef> {
        Some(super::host_group::Relation::Service.def())
    }
}

impl Related<super::service_check::Entity> for Entity {
    fn to() -> RelationDef {
        super::service_check::Relation::Service.def().rev()
    }
}

impl ActiveModelBehavior for ActiveModel {}

#[async_trait]
impl MaremmaEntity for Model {
    #[instrument(level = "debug", skip(_db))]
    async fn find_by_name(name: &str, _db: &DatabaseConnection) -> Result<Option<Model>, Error> {
        Entity::find()
            .filter(Column::Name.eq(name))
            .one(_db)
            .await
            .map_err(Into::into)
    }

    #[instrument(level = "debug", skip_all)]
    async fn update_db_from_config(
        db: &DatabaseConnection,
        config: SendableConfig,
    ) -> Result<(), Error> {
        for (service_name, service) in &config.read().await.services {
            // this is janky but we need to flatten it using serde to get the "extra" fields
            let extra_config: Json = serde_json::to_value(service.extra_config.clone())
                .inspect_err(|err| {
                    error!(
                        "Failed to convert extra_config into JSON for {} error={:?}",
                        service_name, err
                    )
                })?;

            let mut service_value = serde_json::to_value(service).inspect_err(|err| {
                error!(
                    "Failed to convert service into JsonValue for {} error={:?}",
                    service_name, err
                )
            })?;

            if let Some(service_object) = service_value.as_object_mut() {
                if !service_object.contains_key("id")
                    || Some(&serde_json::Value::Null) == service_object.get("id")
                {
                    debug!("Adding ID to service: {}", service_name);
                    if let Some(id) = service_object.get_mut("id") {
                        *id = json!(Uuid::new_v4());
                    } else {
                        return Err(Error::Configuration(format!(
                            "Failed to add ID to service '{}', check the configuration!",
                            service_name
                        )));
                    };
                }
                service_object.insert("name".to_string(), json!(service_name));
                service_object.insert("extra_config".to_string(), json!(extra_config));
            } else {
                error!("Failed to convert service to object: {:?}", service_value);
                return Err(Error::Configuration(format!(
                    "Failed to convert service '{}' to object, check the configuration!",
                    service_name
                )));
            }

            debug!("Looking for {}", service_name);
            // check if we have one and add it if not
            match Entity::find()
                .filter(Column::Name.eq(service_name))
                .one(db)
                .await
            {
                Ok(Some(res)) => {
                    debug!("found it!");
                    let mut res = res.into_active_model();
                    res.name.set_if_not_equals(service_name.clone());
                    if let Err(err) = res.set_from_json(service_value) {
                        error!("Error setting service from json: {:?}", err);
                        return Err(err.into());
                    } else {
                        debug!("Service set from json: {:?}", res);
                    };

                    if res.is_changed() {
                        debug!("Updating service with {:?}", res);
                        res.update(db).await?
                    } else {
                        debug!("try into model");
                        res.try_into_model().inspect_err(|err| {
                            error!("Failed to convert {:?} to model: {:?}", service_name, err)
                        })?
                    }
                }
                Ok(None) => {
                    info!("Didn't find service name='{}' will create it", service_name);
                    // insert the service if we can't find it
                    let mut am = ActiveModel::new();

                    let jsonvalue = serde_json::to_value(&service_value).inspect_err(|err| {
                        error!(
                            "Failed to turn {} into json value? err={:?}",
                            service_name, err
                        )
                    })?;

                    am.set_from_json(jsonvalue.clone()).inspect_err(|err| {
                        error!(
                            "Failed to set model values for {} from JSON {:?} error={:?}",
                            service_name, jsonvalue, err
                        )
                    })?;
                    if am.id.is_not_set() {
                        am.id.set_if_not_equals(Uuid::new_v4());
                    }
                    am.extra_config.set_if_not_equals(json!(extra_config));

                    #[cfg(any(test, debug_assertions))]
                    debug!("about to update this: {:?}", am);

                    debug!("Creating service: {:?}", am);
                    Entity::insert(am).exec_with_returning(db).await?
                }

                Err(err) => return Err(err.into()),
            };
        }

        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn test_service() -> Model {
    Model {
        id: Uuid::new_v4(),
        name: "Test Service".to_string(),
        description: Some("Test Service Description".to_string()),
        service_type: crate::prelude::ServiceType::Cli,
        cron_schedule: "* * * * *".to_string(),
        extra_config: serde_json::json!({ "url": "http://localhost:8080" }).into(),
    }
}

#[cfg(test)]
mod tests {

    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::config::Configuration;
    use crate::db::entities::service_check;
    use crate::db::tests::test_setup;
    use crate::db::{MaremmaEntity, Service, ServiceType};

    use super::*;
    use croner::Cron;
    use sea_orm::ModelTrait;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use serde_json::{json, Value};
    use tokio::sync::RwLock;
    use tracing::info;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_service_entity() {
        let (db, _config) = test_setup().await.expect("Failed to start test harness");

        let db_writer = db.write().await;

        let service = test_service();
        info!("saving service... {:?}", &service);
        let am = service.clone().into_active_model();
        super::Entity::insert(am).exec(&*db_writer).await.unwrap();

        let service = super::Entity::find()
            .filter(super::Column::Id.eq(service.id))
            .one(&*db_writer)
            .await
            .unwrap()
            .unwrap();
        info!("found it: {:?}", service);

        super::Entity::delete_by_id(service.id)
            .exec(&*db_writer)
            .await
            .unwrap();

        assert!(super::Entity::find()
            .filter(super::Column::Id.eq("test_service".to_string()))
            .one(&*db_writer)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_service_update_db_from_config() {
        let (db, _config) = test_setup().await.expect("Failed to start test harness");

        let service = super::Entity::find()
            .filter(super::Column::Name.eq("local_lslah".to_string()))
            .one(&*db.read().await)
            .await
            .unwrap()
            .unwrap();
        info!("found it: {:?}", service);
    }

    #[tokio::test]
    /// Test running config update twice with a service that changes, to ensure it changes.
    async fn test_config_updates() {
        let (db, _config) = test_setup().await.expect("Failed to start test harness");

        let service = super::Entity::find()
            .filter(super::Column::Name.eq("local_lslah".to_string()))
            .one(&*db.read().await)
            .await
            .unwrap()
            .expect("Couldn't find local_lslah");
        info!("found it: {:?}", service);

        let mut config = Configuration::load_test_config_bare().await;

        let extra_config_json = json!({"extra_config" : { "url": "http://localhost:12345" }});
        let extra_config: HashMap<String, Value> =
            serde_json::from_value(extra_config_json.clone())
                .expect("Failed to deserialize JSON into hashmap");

        config.services.insert(
            "local_lslah".to_string(),
            Service::new(
                service.id,
                Some(service.name.clone()),
                Some("New Description".to_string()),
                vec!["test".to_string()],
                ServiceType::Cli,
                Cron::new(&service.cron_schedule)
                    .parse()
                    .expect("couldn't parse cron schedule"),
                extra_config,
            ),
        );

        super::Model::update_db_from_config(&*db.write().await, Arc::new(RwLock::new(config)))
            .await
            .expect("Failed to update db from config");

        let service = super::Entity::find()
            .filter(super::Column::Name.eq("local_lslah".to_string()))
            .one(&*db.read().await)
            .await
            .unwrap()
            .unwrap();
        info!("found it: {:?}", service);
        assert_eq!(service.description, Some("New Description".to_string()));
        assert_eq!(service.extra_config, json!(extra_config_json))
    }

    #[tokio::test]
    async fn test_find_with_linked() {
        let (db, _config) = test_setup().await.expect("Failed to start test harness");

        let (service, groups) = super::Entity::find()
            .find_with_linked(crate::db::entities::service_group_link::ServiceToGroups)
            .all(&*db.read().await)
            .await
            .expect("Failed to run query looking for a service with host groups")
            .into_iter()
            .next()
            .expect("Uh...");

        dbg!(service);
        dbg!(groups);
    }
    #[tokio::test]
    async fn test_find_related_service_to_service_check() {
        let (db, _config) = test_setup().await.expect("Failed to start test harness");

        let db_reader = db.read().await;

        let service = super::Entity::find()
            .one(&*db_reader)
            .await
            .expect("Failed to select service")
            .expect("Failed to find service");

        let service_checks = service
            .find_related(service_check::Entity)
            .all(&*db_reader)
            .await
            .expect("Failed to search for service_checks");

        dbg!(service);
        dbg!(service_checks);
    }

    #[tokio::test]
    async fn test_failing_update_db_from_config_service() {
        use sea_orm::{DatabaseBackend, MockDatabase};

        let db = MockDatabase::new(DatabaseBackend::Sqlite)
            .append_query_results([[super::Model {
                id: Uuid::new_v4(),
                name: "test service".to_string(),
                description: None,
                service_type: ServiceType::Cli,
                cron_schedule: "@hourly".to_string(),
                extra_config: json!({}),
            }]])
            .into_connection();

        let res =
            super::Model::update_db_from_config(&db, Configuration::load_test_config().await).await;

        dbg!(&res);
        assert!(res.is_err());
    }
}
