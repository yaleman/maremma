use sea_orm::entity::prelude::*;

use crate::prelude::ServiceType;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "service")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub host_groups: String,
    #[sea_orm(name = "type")]
    pub type_: ServiceType,
    pub cron_schedule: String,
    pub config: Json,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::service_check::Entity")]
    ServiceCheck,
}

impl Related<super::service_check::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ServiceCheck.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

#[cfg(test)]
pub(crate) fn test_service() -> Model {
    Model {
        id: "test_service".to_string(),
        name: "Test Service".to_string(),
        description: Some("Test Service Description".to_string()),
        host_groups: "test".to_string(),
        type_: crate::prelude::ServiceType::Cli,
        cron_schedule: "* * * * *".to_string(),
        config: serde_json::json!({ "url": "http://localhost:8080" }).into(),
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use tracing::info;

    use super::ActiveModel as ServiceActiveModel;
    use super::Entity as Service;

    use crate::setup_logging;

    #[tokio::test]
    async fn test_service_entity() {
        let _ = setup_logging(true);

        let db = crate::db::test_connect()
            .await
            .expect("Failed to connect to database");

        let service = super::test_service();
        info!("saving service...");
        let am: ServiceActiveModel = service.into();
        Service::insert(am).exec(&db).await.unwrap();

        let service = Service::find()
            .filter(super::Column::Id.eq("test_service".to_string()))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        info!("found it: {:?}", service);

        Service::delete_by_id("test_service".to_string())
            .exec(&db)
            .await
            .unwrap();

        assert!(Service::find()
            .filter(super::Column::Id.eq("test_service".to_string()))
            .one(&db)
            .await
            .unwrap()
            .is_none());
    }
}
