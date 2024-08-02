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
