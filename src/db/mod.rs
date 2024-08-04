use std::process::ExitCode;

use crate::prelude::*;
use migrator::Migrator;
use sea_orm::{Database, DatabaseConnection};
use sea_orm_migration::prelude::*;
use tracing::{info, instrument};

use crate::config::Configuration;

pub mod entities;
pub(crate) mod migrations;
pub(crate) mod migrator;

pub async fn test_connect() -> Result<DatabaseConnection, sea_orm::error::DbErr> {
    let config = Configuration {
        database_file: ":memory:".to_string(),
        ..Default::default()
    };
    connect(&config).await
}

#[instrument(level = "info")]
pub async fn connect(config: &Configuration) -> Result<DatabaseConnection, sea_orm::error::DbErr> {
    let connect_string = if config.database_file == ":memory:" {
        info!("Using in-memory database!");
        "sqlite::memory:".to_string()
    } else {
        format!("sqlite://{}?mode=rwc", config.database_file)
    };

    let db = Database::connect(connect_string).await?;

    Migrator::refresh(&db).await?;
    Ok(db)
}

pub async fn update_db_from_config(
    db: Arc<DatabaseConnection>,
    config: &Configuration,
) -> Result<(), ExitCode> {
    // let's go through and update the DB
    entities::host::Model::update_db_from_config(db.clone(), &config)
        .await
        .map_err(|err| {
            error!("Failed to update hosts DB from config: {:?}", err);
            ExitCode::FAILURE
        })?;
    debug!("Updated hosts");

    entities::host_group::Model::update_db_from_config(db.clone(), &config)
        .await
        .map_err(|err| {
            error!("Failed to update host_groups DB from config: {:?}", err);
            ExitCode::FAILURE
        })?;
    debug!("Updated host_groups");

    entities::host_group_members::Model::update_db_from_config(db.clone(), &config)
        .await
        .map_err(|err| {
            error!(
                "Failed to update host_group_members DB from config: {:?}",
                err
            );
            ExitCode::FAILURE
        })?;
    debug!("Updated host_group_members");

    entities::service::Model::update_db_from_config(db.clone(), &config)
        .await
        .map_err(|err| {
            error!("Failed to update services DB from config: {:?}", err);
            ExitCode::FAILURE
        })?;
    debug!("Updated services");

    entities::service_check::Model::update_db_from_config(db.clone(), &config)
        .await
        .map_err(|err| {
            error!("Failed to update service_checks DB from config: {:?}", err);
            ExitCode::FAILURE
        })?;
    debug!("Updated service checks");

    Ok(())
}
