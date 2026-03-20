use crate::prelude::*;
use entities::host::test_host;
use entities::host_group;
use rand::seq::IteratorRandom;
use sea_orm::{sea_query::Expr, FromQueryResult, JoinType, QuerySelect, Set, TryIntoModel};
use std::str::FromStr;

use super::{host, host_group_members, service, service_check_history, service_group_link};

#[derive(Clone, Debug, Default, PartialEq, Eq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "service_check")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub service_id: Uuid,
    pub host_id: Uuid,
    pub status: ServiceStatus,
    pub last_check: chrono::DateTime<chrono::Utc>,
    pub next_check: chrono::DateTime<chrono::Utc>,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    Service,
    Host,
    ServiceCheckHistory,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::Service => Entity::belongs_to(service::Entity)
                .to(service::Column::Id)
                .from(Column::ServiceId)
                .into(),
            Self::Host => Entity::belongs_to(host::Entity)
                .from(Column::HostId)
                .to(host::Column::Id)
                .into(),
            Self::ServiceCheckHistory => Entity::belongs_to(service_check_history::Entity)
                .from(Column::Id)
                .to(service_check_history::Column::ServiceCheckId)
                .into(),
        }
    }
}

impl Related<service::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Service.def()
    }
}

impl Related<host::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Host.def()
    }
}

impl Related<service_check_history::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ServiceCheckHistory.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    #[instrument(skip(self, db), fields(service_check_id = self.id.hyphenated().to_string(), host_id=self.host_id.hyphenated().to_string()))]
    pub async fn set_status(
        &self,
        status: ServiceStatus,
        db: &DatabaseConnection,
    ) -> Result<Self, Error> {
        let mut model = self.clone().into_active_model();
        model.status.set_if_not_equals(status);
        model.last_updated.set_if_not_equals(chrono::Utc::now());
        model
            .save(db)
            .await
            .map_err(|err| {
                error!(
                    "Failed to set_status service_check_id={}, status={} error={:?}",
                    self.id, status, err
                );
                Error::from(err)
            })?
            .try_into_model()
            .map_err(Error::from)
    }
}

fn next_check_for_service(service: &service::Model, jitter: u32) -> Result<DateTime<Utc>, Error> {
    // Keep next-check calculation in one place so success and error paths schedule consistently.
    let jitter: i64 = (0..jitter).choose(&mut rand::rng()).unwrap_or(0) as i64;

    Ok(
        Cron::from_str(&service.cron_schedule)?.find_next_occurrence(&chrono::Utc::now(), false)?
            + chrono::Duration::seconds(jitter),
    )
}

#[instrument(skip_all, fields(service_check_id = service_check_id.to_string(), status=format!("{}", status)))]
pub async fn set_check_result(
    service_check_id: Uuid,
    service: &service::Model,
    last_check: chrono::DateTime<chrono::Utc>,
    status: ServiceStatus,
    db: &DatabaseConnection,
    jitter: u32,
) -> Result<(), Error> {
    let next_check = next_check_for_service(service, jitter)?;
    let rows_affected = Entity::update_many()
        .col_expr(Column::LastCheck, Expr::value(last_check))
        .col_expr(Column::Status, Expr::value(status))
        .col_expr(Column::NextCheck, Expr::value(next_check))
        .col_expr(Column::LastUpdated, Expr::value(chrono::Utc::now()))
        .filter(Column::Id.eq(service_check_id))
        .exec(db)
        .await
        .map_err(|err| {
            error!(
                "{} error saving service_check result {:?}",
                service.id.hyphenated(),
                err
            );
            Error::from(err)
        })?
        .rows_affected;

    if rows_affected != 1 {
        error!(
            "set_check_result updated {} rows for service_check_id={}",
            rows_affected, service_check_id
        );
    }
    Ok(())
}

async fn update_local_services_from_db(
    db: &DatabaseConnection,
    config: SendableConfig,
) -> Result<(), Error> {
    let local_host_id = match host::Entity::find()
        .filter(host::Column::Hostname.eq(crate::LOCAL_SERVICE_HOST_NAME))
        .one(db)
        .await
        .map_err(Error::from)?
        .map(|h| h.id)
    {
        Some(val) => val,
        None => {
            // local host
            host::Entity::insert(
                host::Model {
                    // setting it to all-zeros makes it clearer it's special
                    id: Uuid::from_u128(0),
                    name: crate::LOCAL_SERVICE_HOST_NAME.to_string(),
                    hostname: crate::LOCAL_SERVICE_HOST_NAME.to_string(),
                    check: crate::host::HostCheck::None,
                    ..test_host()
                }
                .into_active_model(),
            )
            .exec_with_returning(db)
            .await?
            .id
        }
    };

    for service in &config.read().await.local_services.services {
        debug!("Ensuring local service exists: {}", service);
        // can we find the service?

        let service_id = service::Entity::find()
            .filter(service::Column::Name.eq(service))
            .one(db)
            .await
            .map_err(Error::from)?
            .ok_or_else(|| Error::ServiceNotFoundByName(service.clone()))?
            .id;

        // if we can't find it, add it.
        if Entity::find()
            .filter(Column::HostId.eq(local_host_id))
            .filter(Column::ServiceId.eq(service_id))
            .one(db)
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
                    next_check: chrono::Utc::now(),
                    last_updated: chrono::Utc::now(),
                }
                .into_active_model(),
            )
            .exec(db)
            .await
            .map_err(Error::from)?;
        };
    }

    Ok(())
}

#[async_trait]
impl MaremmaEntity for Model {
    async fn find_by_name(_name: &str, _db: &DatabaseConnection) -> Result<Option<Model>, Error> {
        Err(Error::NotImplemented)
    }

    /// This updates all the service checks.
    ///
    /// It needs to be run AFTER you've added all the hosts and services and host_groups!
    async fn update_db_from_config(
        db: &DatabaseConnection,
        config: SendableConfig,
    ) -> Result<(), Error> {
        debug!("Starting update of service checks");
        // the easy ones are the locals.
        info!("Starting local updates...");
        update_local_services_from_db(db, config).await?;

        info!("Starting remote updates...");
        // now we're doing the other services!
        let services: Vec<(service::Model, Vec<host_group::Model>)> = service::Entity::find()
            .find_with_linked(service_group_link::ServiceToGroups)
            .all(db)
            .await?;

        if services.is_empty() {
            error!("No services found, skipping service check update");
            return Ok(());
        } else {
            debug!("Found {} services", services.len());
        }

        for (service, host_groups) in services.into_iter() {
            let service_id = service.id;

            debug!("Checking groups for service: {}", service.name);
            for host_group in host_groups {
                debug!(
                    "Service {} checking group {}",
                    service.name, host_group.name
                );
                // get the group data

                let host_group_members = host_group
                    .find_linked(host_group_members::GroupToHosts)
                    .all(db)
                    .await?;
                for host_group_member in host_group_members {
                    // check if we have the service check
                    match Entity::find()
                        .filter(Column::HostId.eq(host_group_member.id))
                        .filter(Column::ServiceId.eq(service.id))
                        .one(db)
                        .await
                        .map_err(Error::from)?
                    {
                        None => {
                            info!(
                                "Adding service check for service {} on host {:?}",
                                service.name, host_group_member
                            );
                            let model = ActiveModel {
                                id: Set(Uuid::new_v4()),
                                service_id: Set(service_id),
                                host_id: Set(host_group_member.id),
                                status: Set(ServiceStatus::Unknown),
                                last_check: Set(chrono::Utc::now()),
                                next_check: Set(chrono::Utc::now()),
                                last_updated: Set(chrono::Utc::now()),
                            };
                            debug!("Inserting... {:?}", model);
                            model.insert(db).await.map_err(Error::from)?;
                            debug!("Done!");
                        }
                        Some(service_check) => {
                            debug!("Found existing service check: {:?}", service_check);
                        }
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
    pub id: Uuid,
    pub service_name: String,
    pub service_type: ServiceType,
    pub service_id: Uuid,
    pub host_id: Uuid,
    pub host_name: String,

    pub last_check: DateTime<Utc>,
    pub next_check: DateTime<Utc>,
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
            .column_as(service::Column::Id, "service_id")
            .column_as(service::Column::Name, "service_name")
            .column_as(host::Column::Id, "host_id")
            .column_as(host::Column::Hostname, "host_name")
            .column_as(service::Column::ServiceType, "service_type")
            .join(JoinType::LeftJoin, Relation::Service.def())
            .join(JoinType::LeftJoin, Relation::Host.def())
    }

    pub fn get_by_service_id_query(service_id: Uuid) -> Select<Entity> {
        Self::all_query().filter(service::Column::Id.eq(service_id))
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
    use sea_orm::sea_query::Expr;
    use sea_orm::{ColumnTrait, EntityTrait, ModelTrait, QueryFilter};
    use uuid::Uuid;

    use crate::config::Configuration;
    use crate::db::tests::test_setup;
    use crate::db::{entities, MaremmaEntity};
    use crate::errors::Error;
    use crate::services::ServiceStatus;

    #[tokio::test]
    async fn test_find_by_name() {
        // this should error
        let (db, _config) = test_setup().await.expect("Failed to start test harness");

        let res = super::Model::find_by_name("test", db.as_ref()).await;

        assert!(res.is_err());
        assert_eq!(res.expect_err("Failed to run"), Error::NotImplemented);
    }

    #[tokio::test]
    // test that service_checks auto-delete because they're linked to services/hosts via foreign keys
    async fn test_delete_service_checks_when_service_deleted() {
        let (db, _config) = test_setup().await.expect("Failed to start test harness");

        let (service_check, services) = entities::service_check::Entity::find()
            .find_with_related(entities::service::Entity)
            .all(db.as_ref())
            .await
            .expect("Failed to find service")
            .into_iter()
            .next()
            .expect("Failed to get a single service_check");
        let service = services
            .into_iter()
            .next()
            .expect("Failed to get a single service");

        let service_check_id = service_check.id;
        service
            .delete(db.as_ref())
            .await
            .expect("Failed to delete service");

        let res = entities::service_check::Entity::find_by_id(service_check_id)
            .one(db.as_ref())
            .await
            .expect("Failed to find service_check");

        assert!(res.is_none());
    }

    #[tokio::test]
    async fn test_failing_update_db_from_config_service_check() {
        use sea_orm::{DatabaseBackend, MockDatabase};

        let db = MockDatabase::new(DatabaseBackend::Sqlite)
            .append_query_results([[super::Model {
                id: Uuid::new_v4(),
                service_id: Uuid::new_v4(),
                host_id: Uuid::new_v4(),
                status: super::ServiceStatus::Unknown,
                last_check: chrono::Utc::now(),
                next_check: chrono::Utc::now(),
                last_updated: chrono::Utc::now(),
            }]])
            .into_connection();

        let res =
            super::Model::update_db_from_config(&db, Configuration::load_test_config().await).await;

        dbg!(&res);
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_update_db_from_config_does_not_reset_checking() {
        let (db, config) = test_setup().await.expect("Failed to start test harness");

        let service_check = entities::service_check::Entity::find()
            .one(db.as_ref())
            .await
            .expect("Failed to query service check")
            .expect("Failed to find service check");

        service_check
            .set_status(ServiceStatus::Checking, db.as_ref())
            .await
            .expect("Failed to mark service check as checking");

        super::Entity::update_many()
            .col_expr(
                super::Column::LastCheck,
                Expr::value(chrono::Utc::now() - chrono::Duration::minutes(10)),
            )
            .filter(super::Column::Id.eq(service_check.id))
            .exec(db.as_ref())
            .await
            .expect("Failed to age service check");

        super::Model::update_db_from_config(db.as_ref(), config)
            .await
            .expect("Failed to update DB from config");

        let updated = entities::service_check::Entity::find_by_id(service_check.id)
            .one(db.as_ref())
            .await
            .expect("Failed to reload service check")
            .expect("Failed to find updated service check");

        assert_eq!(updated.status, ServiceStatus::Checking);
    }

    #[tokio::test]
    async fn test_from_host_to_service_checks() {
        let (db, _config) = test_setup().await.expect("Failed to start test harness");

        let host = entities::host::Entity::find()
            .one(db.as_ref())
            .await
            .expect("Failed to query db")
            .expect("Failed to find host");

        let service_checks = host
            .find_related(super::Entity)
            .all(db.as_ref())
            .await
            .expect("Failed to query host to service checks relation");

        assert!(!service_checks.is_empty());
    }
}
