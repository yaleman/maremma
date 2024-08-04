#![allow(unused_imports)]

use sea_orm::{Database, FromQueryResult, JoinType, QuerySelect, QueryTrait, TryIntoModel};

use crate::prelude::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "service_check")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub service_id: Uuid,
    pub host_id: Uuid,
    pub status: ServiceStatus,
    pub last_check: chrono::DateTime<chrono::Utc>,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    Service,
    Host,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::Service => Entity::belongs_to(super::service::Entity)
                .from(Column::ServiceId)
                .to(super::service::Column::Id)
                .into(),
            Self::Host => Entity::belongs_to(super::host::Entity)
                .from(Column::HostId)
                .to(super::host::Column::Id)
                .into(),
        }
    }
}

impl ActiveModelBehavior for ActiveModel {}

#[async_trait]
impl MaremmaEntity for Model {
    /// This updates all the service checks. It really needs to be run after you've added all the hosts and services and host_groups!
    async fn update_db_from_config(
        db: Arc<DatabaseConnection>,
        config: &Configuration,
    ) -> Result<(), Error> {
        debug!("Starting update of service checks");
        // the easy ones are the locals.
        let local_host_id = match super::host::Entity::find()
            .filter(super::host::Column::Hostname.eq(crate::LOCAL_SERVICE_HOST_NAME))
            .one(db.as_ref())
            .await
            .map_err(Error::from)?
            .map(|h| h.id)
        {
            Some(val) => val,
            None => {
                super::host::Entity::insert(
                    super::host::Model {
                        id: Uuid::new_v4(),
                        name: crate::LOCAL_SERVICE_HOST_NAME.to_string(),
                        hostname: crate::LOCAL_SERVICE_HOST_NAME.to_string(),
                        check: crate::host::HostCheck::None,
                    }
                    .into_active_model(),
                )
                .exec_with_returning(db.as_ref())
                .await?
                .id
            }
        };

        for service in config.local_services.services.clone() {
            info!("Ensuring local service exists: {}", service);
            // can we find the service?

            let service_id = super::service::Entity::find()
                .filter(super::service::Column::Name.eq(service.as_str()))
                .one(db.as_ref())
                .await
                .map_err(Error::from)?
                .ok_or_else(|| Error::ServiceNotFound)?
                .id;

            // if we can't find it, add it.
            if Entity::find()
                .filter(Column::HostId.eq(local_host_id))
                .filter(Column::ServiceId.eq(service_id))
                .one(db.as_ref())
                .await
                .map_err(Error::from)?
                .is_none()
            {
                debug!("Adding local service check: {}", service);
                Entity::insert(
                    Model {
                        id: Uuid::new_v4(),
                        service_id,
                        host_id: local_host_id,
                        status: ServiceStatus::Unknown,
                        last_check: chrono::Utc::now(),
                        last_updated: chrono::Utc::now(),
                    }
                    .into_active_model(),
                )
                .exec(db.as_ref())
                .await
                .map_err(Error::from)?;
            };
        }

        // now we're doing the other services!
        let services = match super::service::Entity::find().all(db.as_ref()).await {
            Err(DbErr::RecordNotFound(_)) => {
                vec![]
            }
            Ok(services) => services,
            Err(err) => return Err(err.into()),
        };

        if services.is_empty() {
            error!("No services found, skipping service check update");
            return Ok(());
        } else {
            debug!("Found {} services", services.len());
        }

        for service in services.into_iter() {
            debug!("Checking groups for service: {:?}", service.name);
            let host_groups: Vec<String> = match serde_json::from_value(service.host_groups) {
                Ok(host_groups) => host_groups,
                Err(err) => {
                    error!(
                        "Failed to parse host groups for service {}: {}",
                        service.name, err
                    );
                    continue;
                }
            };
            for host_group in host_groups {
                debug!("Service {} checking group {}", service.name, host_group);
                // get the group data
                let group = match super::host_group::Entity::find()
                    .filter(super::host_group::Column::Name.eq(&host_group))
                    .one(db.as_ref())
                    .await
                {
                    Ok(Some(group)) => group,
                    Ok(None) => {
                        error!("Host group {} not found, this should already have been sorted by the update_db_from_config for host_groups", host_group);
                        continue;
                    }
                    Err(err) => {
                        error!("DB Error finding host group {}: {}", host_group, err);
                        continue;
                    }
                };

                let hosts = match super::host_group_members::Entity::find()
                    .filter(super::host_group_members::Column::GroupId.eq(group.id))
                    .all(db.as_ref())
                    .await
                {
                    Ok(hosts) => hosts,
                    Err(err) => {
                        error!("DB Error finding hosts for group {}: {}", host_group, err);
                        return Err(err.into());
                    }
                };
                for host in hosts {
                    // check we have the service check
                    if Entity::find()
                        .filter(Column::HostId.eq(host.host_id))
                        .filter(Column::ServiceId.eq(service.id))
                        .one(db.as_ref())
                        .await
                        .map_err(Error::from)?
                        .is_none()
                    {
                        debug!(
                            "Adding service check for service {} on host {}",
                            service.id, host.host_id
                        );
                        Entity::insert(
                            Model {
                                id: Uuid::new_v4(),
                                service_id: service.id,
                                host_id: host.host_id,
                                status: ServiceStatus::Unknown,
                                last_check: chrono::Utc::now(),
                                last_updated: chrono::Utc::now(),
                            }
                            .into_active_model(),
                        )
                        .exec(db.as_ref())
                        .await
                        .map_err(Error::from)?;
                    }
                }
            }
        }

        Ok(())
    }
}

/// For when you want to see all the details of a service check
#[derive(Clone, Debug, PartialEq, Eq, FromQueryResult)]

pub struct FullServiceCheck {
    pub service_check_id: Uuid,
    pub service_name: String,

    pub host_id: Uuid,
    pub host_name: String,

    pub last_check: DateTime<Utc>,
    pub status: ServiceStatus,
}

impl FullServiceCheck {
    pub async fn all(db: &DatabaseConnection) -> Result<Vec<Self>, Error> {
        Self::all_query()
            .into_model::<FullServiceCheck>()
            .all(db)
            .await
            .map_err(Error::from)
    }

    pub fn all_query() -> Select<Entity> {
        Entity::find()
            .column_as(super::service::Column::Name, "service_name")
            .column_as(super::host::Column::Id, "host_id")
            .column_as(super::host::Column::Hostname, "host_name")
            .join(JoinType::LeftJoin, Relation::Service.def())
            .join(JoinType::LeftJoin, Relation::Host.def())
            .column_as(Column::Id, "service_check_id")
    }

    pub fn get_by_service_id_query(service_id: Uuid) -> Select<Entity> {
        Self::all_query().filter(Column::ServiceId.eq(service_id))
    }

    pub async fn get_by_service_id(
        service_id: Uuid,
        db: &DatabaseConnection,
    ) -> Result<Vec<FullServiceCheck>, Error> {
        Self::get_by_service_id_query(service_id)
            .into_model::<FullServiceCheck>()
            .all(db)
            .await
            .map_err(Error::from)
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;

    use core::panic;
    use std::path::PathBuf;

    use crate::setup_logging;
    use sea_orm::{ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, QueryTrait};

    #[tokio::test]
    async fn test_service_check_entity() {
        let _ = setup_logging(true);

        let db = crate::db::test_connect()
            .await
            .expect("Failed to connect to database");

        let service = super::super::service::test_service();
        let host = super::super::host::test_host();
        info!("saving service...");

        let service_am = service.into_active_model();
        let _service = super::super::service::Entity::insert(service_am.to_owned())
            .exec(&db)
            .await
            .unwrap();
        let host_am = host.into_active_model();
        let _host = super::super::host::Entity::insert(host_am.to_owned())
            .exec(&db)
            .await
            .unwrap();

        let service_check = super::Model {
            id: Uuid::new_v4(),
            service_id: service_am.id.clone().unwrap(),
            host_id: host_am.id.clone().unwrap(),
            ..Default::default()
        };

        let service_check_id = service_check.id;

        let am = service_check.into_active_model();

        if let Err(err) = super::Entity::insert(am).exec(&db).await {
            panic!("Failed to insert service check: {:?}", err);
        };

        let service_check = super::Entity::find()
            .filter(super::Column::Id.eq(service_check_id))
            .one(&db)
            .await
            .unwrap()
            .unwrap();

        info!("found it: {:?}", service_check);

        super::Entity::delete_by_id(service_check_id)
            .exec(&db)
            .await
            .unwrap();
        // Check we didn't delete the host when deleting the service check
        assert!(super::super::host::Entity::find_by_id(host_am.id.unwrap())
            .one(&db)
            .await
            .unwrap()
            .is_some());
        assert!(
            super::super::service::Entity::find_by_id(service_am.id.unwrap())
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

        let service_am = service.into_active_model();
        let _service = super::super::service::Entity::insert(service_am.to_owned())
            .exec(&db)
            .await
            .unwrap();
        let host_am_id = host.id;
        let host_am = host.into_active_model();
        let _host = super::super::host::Entity::insert(host_am.to_owned())
            .exec(&db)
            .await
            .unwrap();

        let service_check = super::Model {
            id: Uuid::new_v4(),
            service_id: service_am.id.unwrap(),
            host_id: host_am.id.unwrap(),
            ..Default::default()
        };
        let service_check_am_id = service_check.id;
        let service_check_am = service_check.into_active_model();
        dbg!(&service_check_am);
        if let Err(err) = super::Entity::insert(service_check_am.to_owned())
            .exec(&db)
            .await
        {
            panic!("Failed to insert service check: {:?}", err);
        };

        assert!(super::Entity::find_by_id(service_check_am.id.unwrap())
            .one(&db)
            .await
            .unwrap()
            .is_some());
        super::super::host::Entity::delete_by_id(host_am_id)
            .exec(&db)
            .await
            .unwrap();
        // Check we delete the service check when deleting the host
        assert!(super::Entity::find_by_id(service_check_am_id)
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

        let service_am = service.clone().into_active_model();
        let _service = super::super::service::Entity::insert(service_am.to_owned())
            .exec(&db)
            .await
            .unwrap();
        let host_am = host.into_active_model();
        let _host = super::super::host::Entity::insert(host_am.clone())
            .exec(&db)
            .await
            .unwrap();

        let service_check = super::Model {
            id: Uuid::new_v4(),
            service_id: service_am.id.unwrap(),
            host_id: host_am.id.unwrap(),
            ..Default::default()
        };
        let service_check_am = service_check.into_active_model();
        dbg!(&service_check_am);
        if let Err(err) = super::Entity::insert(service_check_am.to_owned())
            .exec(&db)
            .await
        {
            panic!("Failed to insert service check: {:?}", err);
        };

        assert!(
            super::Entity::find_by_id(service_check_am.id.clone().unwrap())
                .one(&db)
                .await
                .unwrap()
                .is_some()
        );
        super::super::service::Entity::delete_by_id(service.id)
            .exec(&db)
            .await
            .unwrap();
        // Check we delete the service check when deleting the service
        assert!(super::Entity::find_by_id(service_check_am.id.unwrap())
            .one(&db)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_full_service_check() {
        let _ = setup_logging(true);
        let db = Arc::new(
            crate::db::test_connect()
                .await
                .expect("Failed to connect to database"),
        );

        let configuration =
            crate::config::Configuration::new(Some(PathBuf::from("maremma.example.json")))
                .await
                .expect("Failed to load config");

        crate::db::update_db_from_config(db.clone(), &configuration)
            .await
            .unwrap();

        let known_service_check_service_id = super::Entity::find()
            .all(db.as_ref())
            .await
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .service_id;

        info!(
            "We know we have a service check with service_id: {}",
            known_service_check_service_id
        );

        let query =
            super::FullServiceCheck::get_by_service_id_query(known_service_check_service_id)
                .build((*db).get_database_backend());
        info!("Query: {}", query);

        let service_check =
            super::FullServiceCheck::get_by_service_id(known_service_check_service_id, &db)
                .await
                .expect("Failed to get service_check");

        info!("found service check {:?}", service_check);

        assert!(service_check.len() > 0);
    }
}
