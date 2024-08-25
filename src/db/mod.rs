#![allow(missing_docs)]

use crate::prelude::*;
use migrator::Migrator;
use sea_orm::{Database, DatabaseConnection, QueryOrder, TransactionTrait};
use sea_orm_migration::prelude::*;
use tracing::{info, instrument};

use crate::config::Configuration;

pub mod entities;
pub(crate) mod migrations;
pub(crate) mod migrator;
#[cfg(test)]
pub(crate) mod tests;

pub async fn test_connect() -> Result<DatabaseConnection, sea_orm::error::DbErr> {
    let config = Configuration {
        database_file: ":memory:".to_string(),
        ..Default::default()
    };
    connect(&config).await
}

#[instrument(level = "info", skip_all)]
pub async fn connect(config: &Configuration) -> Result<DatabaseConnection, sea_orm::error::DbErr> {
    let connect_string = if config.database_file == ":memory:" {
        info!("Using in-memory database!");
        "sqlite::memory:".to_string()
    } else {
        format!("sqlite://{}?mode=rwc", config.database_file)
    };

    let db = Database::connect(connect_string).await?;
    // start a transaction so if it doesn't work, we can roll back.
    let db_transaction = db.begin().await?;
    Migrator::up(&db_transaction, None).await?;
    db_transaction.commit().await?;

    Ok(db)
}

pub async fn update_db_from_config(
    db: &DatabaseConnection,
    config: Arc<Configuration>,
) -> Result<(), Error> {
    // let's go through and update the DB

    entities::host::Model::update_db_from_config(db, config.clone())
        .await
        .inspect_err(|err| {
            error!("Failed to update hosts DB from config: {:?}", err);
        })?;
    info!("Updated hosts");

    entities::host_group::Model::update_db_from_config(db, config.clone())
        .await
        .inspect_err(|err| {
            error!("Failed to update host_groups DB from config: {:?}", err);
        })?;
    info!("Updated host_groups");

    entities::host_group_members::Model::update_db_from_config(db, config.clone())
        .await
        .inspect_err(|err| {
            error!(
                "Failed to update host_group_members DB from config: {:?}",
                err
            );
        })?;
    info!("Updated host_group_members");

    entities::service::Model::update_db_from_config(db, config.clone())
        .await
        .inspect_err(|err| {
            error!("Failed to update services DB from config: {:?}", err);
        })?;
    info!("Updated services");

    entities::service_check::Model::update_db_from_config(db, config.clone())
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
) -> Result<
    Option<(
        entities::service_check::Model,
        Option<entities::service::Model>,
    )>,
    Error,
> {
    let base_query =
        entities::service_check::Entity::find().find_also_related(entities::service::Entity);

    let urgent = base_query
        .clone()
        .filter(entities::service_check::Column::Status.eq(ServiceStatus::Urgent))
        // oldest-last-updated is the most urgent
        .order_by_asc(entities::service_check::Column::LastUpdated)
        .one(db)
        .await?;

    if let Some(row) = urgent {
        return Ok(Some(row));
    }

    // all others we just care about:
    // - the next_check time
    let base_query = base_query
        .order_by_asc(entities::service_check::Column::NextCheck)
        .filter(
            entities::service_check::Column::Status
                .ne(ServiceStatus::Disabled)
                .and(entities::service_check::Column::Status.ne(ServiceStatus::Checking))
                .and(entities::service_check::Column::NextCheck.lte(chrono::Utc::now())),
        );

    // prioritize pending
    if let Some(res) = base_query
        .clone()
        .filter(entities::service_check::Column::Status.eq(ServiceStatus::Pending))
        .one(db)
        .await?
    {
        return Ok(Some(res));
    }

    Ok(base_query.one(db).await?.into_iter().next())
}
