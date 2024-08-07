use sea_orm::entity::prelude::*;
use sea_orm::{Set, TryIntoModel};

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
    #[sea_orm(name = "type")]
    #[serde(alias = "type")]
    pub type_: ServiceType,
    pub cron_schedule: String,
    #[serde(flatten)]
    pub extra_config: Option<Json>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl Related<super::host::Entity> for Entity {
    fn to() -> RelationDef {
        super::service_check::Relation::Host.def().rev()
    }

    fn via() -> Option<RelationDef> {
        Some(super::service_check::Relation::Service.def())
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
    async fn update_db_from_config(
        db: Arc<DatabaseConnection>,
        config: Arc<Configuration>,
    ) -> Result<(), Error> {
        if let Some(services) = config.services.clone() {
            if let Some(services) = services.as_object() {
                for (service_name, service) in services {
                    let mut service = service.to_owned();

                    // this is janky but we need to flatten it using serde to get the "extra" fields
                    let service_parsed: Service = serde_json::from_value(service.clone())?;
                    let extra_config: Json = serde_json::to_value(service_parsed.extra_config)?;

                    if let Some(service_object) = service.as_object_mut() {
                        service_object.insert("id".to_string(), json!(Uuid::new_v4()));
                        service_object.insert("name".to_string(), json!(service_name));
                        service_object.insert("extra_config".to_string(), json!(extra_config));
                    }

                    // check if we have one and add it if not
                    match Entity::find()
                        .filter(Column::Name.eq(service_name))
                        .one(db.as_ref())
                        .await
                    {
                        Ok(Some(res)) => {
                            let mut res = res.into_active_model();

                            res.set_from_json(service.clone())?;
                            if res.is_changed() {
                                debug!("about to update this: {:?}", res);
                                debug!("Source: {:?}", service);
                                res.update(db.as_ref()).await?
                            } else {
                                res.try_into_model()?
                            }
                        }
                        Ok(None) => {
                            // insert the service if we can't find it
                            let mut am = ActiveModel::new();
                            let service_id = Uuid::new_v4();

                            am.set_from_json(service)?;
                            am.id = Set(service_id);
                            debug!("Creating service: {:?}", am);
                            Entity::insert(am).exec_with_returning(&*db).await?
                        }

                        Err(err) => return Err(err.into()),
                    };
                }
            } else {
                return Err(Error::ConfigParse(
                    "services in configuration is not an object!".to_string(),
                ));
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
        type_: crate::prelude::ServiceType::Cli,
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
        info!("saving service...");
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
