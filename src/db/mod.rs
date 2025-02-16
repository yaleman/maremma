#![allow(missing_docs)]

use crate::prelude::*;
use migrator::Migrator;
use sea_orm::{
    ConnectOptions, Database, DatabaseConnection, QueryOrder, QuerySelect, TransactionTrait,
};
use sea_orm_migration::prelude::*;
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
        format!("sqlite://{}?mode=rwc", database_file)
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
pub async fn update_db_from_config(
    db_writer: &DatabaseConnection,
    config: SendableConfig,
) -> Result<(), Error> {
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
    info!("Updated host_groups");

    entities::host_group_members::Model::update_db_from_config(db_writer, config.clone())
        .await
        .inspect_err(|err| {
            error!(
                "Failed to update host_group_members DB from config: {:?}",
                err
            );
        })?;
    info!("Updated host_group_members");

    entities::service::Model::update_db_from_config(db_writer, config.clone())
        .await
        .inspect_err(|err| {
            error!("Failed to update services DB from config: {:?}", err);
        })?;
    info!("Updated services");

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

    Ok(())
}

/// Get the next service check to run, returns
pub async fn get_next_service_check(
    db: &DatabaseConnection,
) -> Result<Option<(entities::service_check::Model, entities::service::Model)>, Error> {
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
                    .and(entities::service_check::Column::NextCheck.lte(chrono::Utc::now())),
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
                Error::Generic("Failed to get service for service check".to_string())
            })?;
            Ok(Some((service_check.to_owned(), service)))
        }
        None => Ok(None),
    }
}
