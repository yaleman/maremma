use sea_orm::entity::prelude::*;
use sea_orm::Set;

use crate::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "service")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, name = "id")]
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    /// should be a list of strings
    pub host_groups: Json,
    #[sea_orm(name = "type")]
    #[serde(alias = "type")]
    pub type_: ServiceType,
    pub cron_schedule: String,
    #[serde(flatten)]
    pub config: Option<Json>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl Related<super::host::Entity> for Entity {
    fn to() -> RelationDef {
        super::service_check::Relation::Service.def()
    }

    fn via() -> Option<RelationDef> {
        Some(super::service_check::Relation::Host.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}

#[async_trait]
impl MaremmaEntity for Model {
    async fn update_db_from_config(
        db: Arc<DatabaseConnection>,
        config: &Configuration,
    ) -> Result<(), Error> {
        if let Some(services) = config.services.clone() {
            if let Some(services) = services.as_object() {
                for (service_name, service) in services {
                    // check if we have one and add it if not
                    match Entity::find()
                        .filter(Column::Name.eq(service_name))
                        .one(db.as_ref())
                        .await
                    {
                        Ok(Some(res)) => res,
                        Ok(None) | Err(DbErr::RecordNotFound(_)) => {
                            // insert the service if we can't find it
                            let mut am = ActiveModel::new();
                            let mut service = service.to_owned();
                            let service_id = Uuid::new_v4();
                            if let Some(service_object) = service.as_object_mut() {
                                service_object.insert("id".to_string(), json!(Uuid::new_v4()));
                                service_object.insert("name".to_string(), json!(service_name));
                            }

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
        config: serde_json::json!({ "url": "http://localhost:8080" }).into(),
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;

    use sea_orm::IntoActiveModel;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use tracing::info;

    use crate::setup_logging;

    #[tokio::test]
    async fn test_service_entity() {
        let _ = setup_logging(true);

        let db = crate::db::test_connect()
            .await
            .expect("Failed to connect to database");

        let service = super::test_service();
        info!("saving service...");
        let am = service.clone().into_active_model();
        super::Entity::insert(am).exec(&db).await.unwrap();

        let service = super::Entity::find()
            .filter(super::Column::Id.eq(service.id))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        info!("found it: {:?}", service);

        super::Entity::delete_by_id(service.id)
            .exec(&db)
            .await
            .unwrap();

        assert!(super::Entity::find()
            .filter(super::Column::Id.eq("test_service".to_string()))
            .one(&db)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_service_update_db_from_config() {
        let _ = setup_logging(true);

        let db = Arc::new(
            crate::db::test_connect()
                .await
                .expect("Failed to connect to database"),
        );

        let config = Configuration::load_test_config().await;
        super::Model::update_db_from_config(db.clone(), &config)
            .await
            .unwrap();

        let service = super::Entity::find()
            .filter(super::Column::Name.eq("local_lslah".to_string()))
            .one(db.as_ref())
            .await
            .unwrap()
            .unwrap();
        info!("found it: {:?}", service);
    }
}
