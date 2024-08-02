use sea_orm::entity::prelude::*;

use crate::prelude::ServiceStatus;

#[derive(Clone, Debug, Default, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "service_check")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: String,
    pub service_id: String,
    pub host_id: String,
    pub status: ServiceStatus,
    pub last_check: chrono::DateTime<chrono::Utc>,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_one = "super::service::Entity")]
    Service,
    #[sea_orm(has_one = "super::host::Entity")]
    Host,
}

impl Related<super::service::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Service.def()
    }
}
impl Related<super::host::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Host.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

#[cfg(test)]
mod tests {
    use core::panic;

    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use tracing::info;

    use crate::setup_logging;

    #[tokio::test]
    async fn test_service_check_entity() {
        let _ = setup_logging(true);

        let db = crate::db::test_connect()
            .await
            .expect("Failed to connect to database");

        let service = super::super::service::test_service();
        let host = super::super::host::test_host();
        info!("saving service...");

        let service_am: super::super::service::ActiveModel = service.into();
        let _service = super::super::service::Entity::insert(service_am.to_owned())
            .exec(&db)
            .await
            .unwrap();
        let host_am: super::super::host::ActiveModel = host.into();
        let _host = super::super::host::Entity::insert(host_am.to_owned())
            .exec(&db)
            .await
            .unwrap();

        let service_check = super::Model {
            id: "test_service_check".into(),
            service_id: service_am.id.as_ref().to_owned(),
            host_id: host_am.id.as_ref().to_owned(),
            ..Default::default()
        };

        let am: super::ActiveModel = service_check.into();
        dbg!(&am);
        if let Err(err) = super::Entity::insert(am).exec(&db).await {
            panic!("Failed to insert service check: {:?}", err);
        };

        let service_check = super::Entity::find()
            .filter(super::Column::Id.eq("test_service_check".to_string()))
            .one(&db)
            .await
            .unwrap()
            .unwrap();

        info!("found it: {:?}", service_check);

        super::Entity::delete_by_id("test_service_check".to_string())
            .exec(&db)
            .await
            .unwrap();
        // Check we didn't delete the host when deleting the service check
        assert!(super::super::host::Entity::find_by_id(host_am.id.as_ref())
            .one(&db)
            .await
            .unwrap()
            .is_some());
        assert!(
            super::super::service::Entity::find_by_id(service_am.id.as_ref())
                .one(&db)
                .await
                .unwrap()
                .is_some()
        );

        // TODO: test creating a service + host + service check, then deleting a service - which should delete the service_check
    }

    #[tokio::test]
    /// test creating a service + host + service check, then deleting a host - which should delete the service_check
    async fn test_service_check_fk_host() {
        let _ = setup_logging(true);

        let db = crate::db::test_connect()
            .await
            .expect("Failed to connect to database");

        let service = super::super::service::test_service();
        let host = super::super::host::test_host();
        info!("saving service...");

        let service_am: super::super::service::ActiveModel = service.into();
        let _service = super::super::service::Entity::insert(service_am.to_owned())
            .exec(&db)
            .await
            .unwrap();
        let host_am: super::super::host::ActiveModel = host.into();
        let _host = super::super::host::Entity::insert(host_am.to_owned())
            .exec(&db)
            .await
            .unwrap();

        let service_check = super::Model {
            id: "test_service_check".into(),
            service_id: service_am.id.as_ref().to_owned(),
            host_id: host_am.id.as_ref().to_owned(),
            ..Default::default()
        };

        let service_check_am: super::ActiveModel = service_check.into();
        dbg!(&service_check_am);
        if let Err(err) = super::Entity::insert(service_check_am.to_owned())
            .exec(&db)
            .await
        {
            panic!("Failed to insert service check: {:?}", err);
        };

        assert!(super::Entity::find_by_id(service_check_am.id.as_ref())
            .one(&db)
            .await
            .unwrap()
            .is_some());
        super::super::host::Entity::delete_by_id(host_am.id.as_ref())
            .exec(&db)
            .await
            .unwrap();
        // Check we delete the service check when deleting the host
        assert!(super::Entity::find_by_id(service_check_am.id.as_ref())
            .one(&db)
            .await
            .unwrap()
            .is_none());
    }
    #[tokio::test]
    /// test creating a service + host + service check, then deleting a host - which should delete the service_check
    async fn test_service_check_fk_service() {
        let _ = setup_logging(true);

        let db = crate::db::test_connect()
            .await
            .expect("Failed to connect to database");

        let service = super::super::service::test_service();
        let host = super::super::host::test_host();
        info!("saving service...");

        let service_am: super::super::service::ActiveModel = service.into();
        let _service = super::super::service::Entity::insert(service_am.to_owned())
            .exec(&db)
            .await
            .unwrap();
        let host_am: super::super::host::ActiveModel = host.into();
        let _host = super::super::host::Entity::insert(host_am.to_owned())
            .exec(&db)
            .await
            .unwrap();

        let service_check = super::Model {
            id: "test_service_check".into(),
            service_id: service_am.id.as_ref().to_owned(),
            host_id: host_am.id.as_ref().to_owned(),
            ..Default::default()
        };

        let service_check_am: super::ActiveModel = service_check.into();
        dbg!(&service_check_am);
        if let Err(err) = super::Entity::insert(service_check_am.to_owned())
            .exec(&db)
            .await
        {
            panic!("Failed to insert service check: {:?}", err);
        };

        assert!(super::Entity::find_by_id(service_check_am.id.as_ref())
            .one(&db)
            .await
            .unwrap()
            .is_some());
        super::super::service::Entity::delete_by_id(service_am.id.as_ref())
            .exec(&db)
            .await
            .unwrap();
        // Check we delete the service check when deleting the service
        assert!(super::Entity::find_by_id(service_check_am.id.as_ref())
            .one(&db)
            .await
            .unwrap()
            .is_none());
    }
}
