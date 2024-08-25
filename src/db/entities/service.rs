use sea_orm::entity::prelude::*;
use sea_orm::TryIntoModel;

use crate::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "service")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, name = "id")]
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    /// A list of host group names
    pub host_groups: Json,
    pub service_type: ServiceType,
    pub cron_schedule: String,
    #[serde(flatten)]
    pub extra_config: Option<Json>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::host_group::Entity")]
    HostGroup,
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
        super::service_check::Relation::Service.def()
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
        if let Some(services) = config.services.as_ref() {
            for (service_name, service) in services {
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
                        };

                        if res.is_changed() {
                            #[cfg(any(test, debug_assertions))]
                            {
                                eprintln!("about to update this: {:?}", res);
                                eprintln!("Source: {:?}", service);
                            }
                            res.update(db).await?
                        } else {
                            eprintln!("try into model");
                            res.try_into_model().inspect_err(|err| {
                                error!("Failed to convert {:?} to model: {:?}", service_name, err)
                            })?
                        }
                    }
                    Ok(None) => {
                        debug!("didn't find it!");
                        // insert the service if we can't find it
                        let mut am = ActiveModel::new();

                        let jsonvalue =
                            serde_json::to_value(&service_value).inspect_err(|err| {
                                error!("Failed to turn thing into json value? err={:?}", err)
                            })?;

                        am.set_from_json(jsonvalue.clone()).inspect_err(|err| {
                            error!(
                                "Failed to set model values from JSON {:?} error={:?}",
                                jsonvalue, err
                            )
                        })?;
                        am.id.set_if_not_equals(Uuid::new_v4());
                        #[cfg(any(test, debug_assertions))]
                        eprintln!("about to update this: {:?}", am);

                        debug!("Creating service: {:?}", am);
                        Entity::insert(am).exec_with_returning(db).await?
                    }

                    Err(err) => return Err(err.into()),
                };
            }
        } else {
            error!("No services in config!");
        }

        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn test_service() -> Model {
    use serde_json::json;

    Model {
        id: Uuid::new_v4(),
        name: "Test Service".to_string(),
        description: Some("Test Service Description".to_string()),
        host_groups: json! {["test".to_string()]},
        service_type: crate::prelude::ServiceType::Cli,
        cron_schedule: "* * * * *".to_string(),
        extra_config: serde_json::json!({ "url": "http://localhost:8080" }).into(),
    }
}

#[cfg(test)]
mod tests {
    use crate::db::tests::test_setup;

    use sea_orm::IntoActiveModel;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use tracing::info;

    #[tokio::test]
    async fn test_service_entity() {
        let (db, _config) = test_setup().await.expect("Failed to start test harness");

        let service = super::test_service();
        info!("saving service... {:?}", &service);
        let am = service.clone().into_active_model();
        super::Entity::insert(am).exec(db.as_ref()).await.unwrap();

        let service = super::Entity::find()
            .filter(super::Column::Id.eq(service.id))
            .one(db.as_ref())
            .await
            .unwrap()
            .unwrap();
        info!("found it: {:?}", service);

        super::Entity::delete_by_id(service.id)
            .exec(db.as_ref())
            .await
            .unwrap();

        assert!(super::Entity::find()
            .filter(super::Column::Id.eq("test_service".to_string()))
            .one(db.as_ref())
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_service_update_db_from_config() {
        let (db, _config) = test_setup().await.expect("Failed to start test harness");

        let service = super::Entity::find()
            .filter(super::Column::Name.eq("local_lslah".to_string()))
            .one(db.as_ref())
            .await
            .unwrap()
            .unwrap();
        info!("found it: {:?}", service);
    }
}
