#![allow(missing_docs)]

use crate::prelude::*;
use migrator::Migrator;
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, QueryOrder, QuerySelect,
    TransactionTrait,
};
use sea_orm_migration::prelude::*;
use std::collections::{HashMap, HashSet};
use tracing::{info, instrument};

use crate::config::Configuration;

pub mod entities;
pub(crate) mod migrations;
pub(crate) mod migrator;
#[cfg(test)]
pub(crate) mod tests;

pub async fn test_connect() -> Result<DatabaseConnection, sea_orm::error::DbErr> {
    let config = Arc::new(RwLock::new(Configuration {
        database_file: ":memory:".to_string(),
        ..Default::default()
    }));
    connect(config).await
}

pub async fn get_connect_string(config: SendableConfig) -> String {
    let database_file = config.read().await.database_file.clone();

    if database_file == ":memory:" {
        info!("Using in-memory database!");
        "sqlite::memory:".to_string()
    } else {
        format!("sqlite://{database_file}?mode=rwc")
    }
}

#[instrument(level = "info", skip_all)]
pub async fn connect(config: SendableConfig) -> Result<DatabaseConnection, sea_orm::error::DbErr> {
    let mut connect_options = ConnectOptions::new(get_connect_string(config).await);
    connect_options
        .sqlx_slow_statements_logging_settings(
            log::LevelFilter::Warn,
            std::time::Duration::from_secs(2),
        )
        .acquire_timeout(std::time::Duration::from_secs(10));

    let db = Database::connect(connect_options).await?;
    // start a transaction so if it doesn't work, we can roll back.
    let db_transaction = db.begin().await?;
    Migrator::up(&db_transaction, None).await?;
    db_transaction.commit().await?;
    Ok(db)
}

#[instrument(level = "debug", skip_all)]
pub async fn update_db_from_config<C>(
    db_writer: &C,
    config: SendableConfig,
) -> Result<(), MaremmaError>
where
    C: ConnectionTrait + Sync,
{
    // let's go through and update the DB
    entities::host::Model::update_db_from_config(db_writer, config.clone())
        .await
        .inspect_err(|err| {
            error!("Failed to update hosts DB from config: {:?}", err);
        })?;
    info!("Updated hosts");

    entities::host_group::Model::update_db_from_config(db_writer, config.clone())
        .await
        .inspect_err(|err| {
            error!("Failed to update host_groups DB from config: {:?}", err);
        })?;
    debug!("Updated host_groups");

    entities::host_group_members::Model::update_db_from_config(db_writer, config.clone())
        .await
        .inspect_err(|err| {
            error!(
                "Failed to update host_group_members DB from config: {:?}",
                err
            );
        })?;
    debug!("Updated host_group_members");

    entities::service::Model::update_db_from_config(db_writer, config.clone())
        .await
        .inspect_err(|err| {
            error!("Failed to update services DB from config: {:?}", err);
        })?;
    debug!("Updated services");

    entities::service_group_link::Model::update_db_from_config(db_writer, config.clone())
        .await
        .inspect_err(|err| {
            error!(
                "Failed to update service_group_links DB from config: {:?}",
                err
            );
        })?;

    entities::service_check::Model::update_db_from_config(db_writer, config.clone())
        .await
        .inspect_err(|err| {
            error!("Failed to update service_checks DB from config: {:?}", err);
        })?;
    info!("Updated service checks");

    prune_db_to_config(db_writer, config).await?;
    info!("Pruned database records absent from configuration");

    Ok(())
}

async fn prune_db_to_config<C>(db: &C, config: SendableConfig) -> Result<(), MaremmaError>
where
    C: ConnectionTrait + Sync,
{
    let config = config.read().await;
    let configured_host_names = config.hosts.keys().cloned().collect::<HashSet<_>>();
    let configured_service_names = config.services.keys().cloned().collect::<HashSet<_>>();
    let configured_group_names = config.groups().into_iter().collect::<HashSet<_>>();

    let hosts = entities::host::Entity::find().all(db).await?;
    let services = entities::service::Entity::find().all(db).await?;
    let groups = entities::host_group::Entity::find().all(db).await?;

    let host_ids = hosts
        .iter()
        .map(|host| (host.name.clone(), host.id))
        .collect::<HashMap<_, _>>();
    let service_ids = services
        .iter()
        .map(|service| (service.name.clone(), service.id))
        .collect::<HashMap<_, _>>();
    let group_ids = groups
        .iter()
        .map(|group| (group.name.clone(), group.id))
        .collect::<HashMap<_, _>>();

    let mut configured_memberships = HashSet::new();
    for (host_name, host) in &config.hosts {
        let host_id = host_ids.get(host_name).ok_or_else(|| {
            MaremmaError::Configuration(format!(
                "Host '{host_name}' was not stored while reconciling configuration"
            ))
        })?;
        for group_name in &host.host_groups {
            let group_id = group_ids
                .get(group_name)
                .ok_or_else(|| MaremmaError::HostGroupNotFoundByName(group_name.to_string()))?;
            configured_memberships.insert((*host_id, *group_id));
        }
    }

    let mut configured_service_links = HashSet::new();
    let mut configured_checks = HashSet::new();
    for (service_name, service) in &config.services {
        let service_id = service_ids
            .get(service_name)
            .ok_or_else(|| MaremmaError::ServiceNotFoundByName(service_name.to_string()))?;
        for group_name in &service.host_groups {
            let group_id = group_ids
                .get(group_name)
                .ok_or_else(|| MaremmaError::HostGroupNotFoundByName(group_name.to_string()))?;
            configured_service_links.insert((*service_id, *group_id));

            for (host_name, host) in &config.hosts {
                if host.host_groups.contains(group_name) {
                    let host_id = host_ids.get(host_name).ok_or_else(|| {
                        MaremmaError::Configuration(format!(
                            "Host '{host_name}' was not stored while reconciling checks"
                        ))
                    })?;
                    configured_checks.insert((*host_id, *service_id));
                }
            }
        }
    }

    let local_host_id = host_ids.get(LOCAL_SERVICE_HOST_NAME);
    for service_name in &config.local_services.services {
        let host_id = local_host_id.ok_or_else(|| {
            MaremmaError::Configuration("Local service host was not stored".to_string())
        })?;
        let service_id = service_ids
            .get(service_name)
            .ok_or_else(|| MaremmaError::ServiceNotFoundByName(service_name.to_string()))?;
        configured_checks.insert((*host_id, *service_id));
    }
    drop(config);

    let stale_check_ids = entities::service_check::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .filter(|check| !configured_checks.contains(&(check.host_id, check.service_id)))
        .map(|check| check.id)
        .collect::<Vec<_>>();
    if !stale_check_ids.is_empty() {
        entities::service_check::Entity::delete_many()
            .filter(entities::service_check::Column::Id.is_in(stale_check_ids))
            .exec(db)
            .await?;
    }

    let stale_service_link_ids = entities::service_group_link::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .filter(|link| !configured_service_links.contains(&(link.service_id, link.group_id)))
        .map(|link| link.id)
        .collect::<Vec<_>>();
    if !stale_service_link_ids.is_empty() {
        entities::service_group_link::Entity::delete_many()
            .filter(entities::service_group_link::Column::Id.is_in(stale_service_link_ids))
            .exec(db)
            .await?;
    }

    let stale_membership_ids = entities::host_group_members::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .filter(|membership| {
            !configured_memberships.contains(&(membership.host_id, membership.group_id))
        })
        .map(|membership| membership.id)
        .collect::<Vec<_>>();
    if !stale_membership_ids.is_empty() {
        entities::host_group_members::Entity::delete_many()
            .filter(entities::host_group_members::Column::Id.is_in(stale_membership_ids))
            .exec(db)
            .await?;
    }

    let stale_host_ids = hosts
        .into_iter()
        .filter(|host| !configured_host_names.contains(&host.name))
        .map(|host| host.id)
        .collect::<Vec<_>>();
    if !stale_host_ids.is_empty() {
        entities::host::Entity::delete_many()
            .filter(entities::host::Column::Id.is_in(stale_host_ids))
            .exec(db)
            .await?;
    }

    let stale_service_ids = services
        .into_iter()
        .filter(|service| !configured_service_names.contains(&service.name))
        .map(|service| service.id)
        .collect::<Vec<_>>();
    if !stale_service_ids.is_empty() {
        entities::service::Entity::delete_many()
            .filter(entities::service::Column::Id.is_in(stale_service_ids))
            .exec(db)
            .await?;
    }

    let stale_group_ids = groups
        .into_iter()
        .filter(|group| !configured_group_names.contains(&group.name))
        .map(|group| group.id)
        .collect::<Vec<_>>();
    if !stale_group_ids.is_empty() {
        entities::host_group::Entity::delete_many()
            .filter(entities::host_group::Column::Id.is_in(stale_group_ids))
            .exec(db)
            .await?;
    }

    Ok(())
}

/// Get the next service check to run, returns
pub async fn get_next_service_check(
    db: &DatabaseConnection,
) -> Result<Option<(entities::service_check::Model, entities::service::Model)>, MaremmaError> {
    let base_query =
        entities::service_check::Entity::find().find_with_related(entities::service::Entity);

    let mut res = base_query
        .clone()
        .filter(entities::service_check::Column::Status.eq(ServiceStatus::Urgent))
        // TODO: test the whole "which next check gets picked if they're both urgent"
        // oldest-next-check is the most urgent
        .order_by_asc(entities::service_check::Column::NextCheck)
        .limit(1)
        .all(db)
        .await?
        .into_iter()
        .next();

    // prioritize pending
    if res.is_none() {
        // all others we just care about the next_check time
        let base_query = base_query
            .order_by_asc(entities::service_check::Column::NextCheck)
            .filter(
                entities::service_check::Column::Status
                    .ne(ServiceStatus::Disabled)
                    .and(entities::service_check::Column::Status.ne(ServiceStatus::Checking))
                    .and(entities::service_check::Column::NextCheck.lte(Utc::now())),
            )
            .distinct();
        // check for pending ones
        res = match base_query
            .clone()
            .filter(entities::service_check::Column::Status.eq(ServiceStatus::Pending))
            .limit(1)
            .all(db)
            .await?
            .into_iter()
            .next()
        {
            Some(row) => Some(row),
            None => {
                // fall through to whatever
                base_query.all(db).await?.into_iter().next()
            }
        };
    }

    match res {
        Some((service_check, mut services)) => {
            let service = services.pop().ok_or_else(|| {
                MaremmaError::Generic("Failed to get service for service check".to_string())
            })?;
            Ok(Some((service_check.to_owned(), service)))
        }
        None => Ok(None),
    }
}
