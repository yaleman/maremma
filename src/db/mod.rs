use std::process::ExitCode;

use crate::prelude::*;
use migrator::Migrator;
use sea_orm::{Database, DatabaseConnection, QueryOrder, QuerySelect};
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
    entities::host::Model::update_db_from_config(db.clone(), config)
        .await
        .map_err(|err| {
            error!("Failed to update hosts DB from config: {:?}", err);
            ExitCode::FAILURE
        })?;
    debug!("Updated hosts");

    entities::host_group::Model::update_db_from_config(db.clone(), config)
        .await
        .map_err(|err| {
            error!("Failed to update host_groups DB from config: {:?}", err);
            ExitCode::FAILURE
        })?;
    debug!("Updated host_groups");

    entities::host_group_members::Model::update_db_from_config(db.clone(), config)
        .await
        .map_err(|err| {
            error!(
                "Failed to update host_group_members DB from config: {:?}",
                err
            );
            ExitCode::FAILURE
        })?;
    debug!("Updated host_group_members");

    entities::service::Model::update_db_from_config(db.clone(), config)
        .await
        .map_err(|err| {
            error!("Failed to update services DB from config: {:?}", err);
            ExitCode::FAILURE
        })?;
    debug!("Updated services");

    entities::service_check::Model::update_db_from_config(db.clone(), config)
        .await
        .map_err(|err| {
            error!("Failed to update service_checks DB from config: {:?}", err);
            ExitCode::FAILURE
        })?;
    debug!("Updated service checks");

    Ok(())
}

/// find the next time we need to wake up
pub async fn find_next_wakeup(_db: &DatabaseConnection) -> DateTime<Utc> {
    // let mut next_wakeup: Option<DateTime<Utc>> = None;

    // for (_id, check) in self.service_checks.read().await.iter() {
    //     if let Ok(cron) = check.get_cron(self) {
    //         if let Ok(next_runtime) = cron.find_next_occurrence(&check.last_check, false) {
    //             match next_wakeup {
    //                 Some(wakeup) => {
    //                     if next_runtime < wakeup {
    //                         next_wakeup = Some(next_runtime);
    //                     }
    //                 }
    //                 None => {
    //                     next_wakeup = Some(next_runtime);
    //                 }
    //             }
    //         }
    //     }
    // }
    // next_wakeup.unwrap_or(chrono::Utc::now() + TimeDelta::seconds(1))
    chrono::Utc::now() + TimeDelta::seconds(1)
}

/// Get the next service check to run, returns
pub async fn get_next_service_check(
    db: &DatabaseConnection,
) -> Result<Option<entities::service_check::Model>, Error> {
    let urgent = entities::service_check::Entity::find()
        .filter(entities::service_check::Column::Status.eq(ServiceStatus::Urgent))
        // oldest-last-updated is the most urgent
        .order_by_asc(entities::service_check::Column::LastUpdated)
        .limit(1)
        .all(db)
        .await?;

    if let Some(model) = urgent.into_iter().next() {
        return Ok(Some(model));
    }
    // prioritize pending

    if let Some(res) = entities::service_check::Entity::find()
        .filter(entities::service_check::Column::Status.ne(ServiceStatus::Disabled))
        .all(db)
        .await?
        .into_iter()
        .next()
    {
        return Ok(Some(res));
    }

    Ok(entities::service_check::Entity::find()
        .filter(entities::service_check::Column::Status.ne(ServiceStatus::Disabled))
        .all(db)
        .await?
        .into_iter()
        .next())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::setup_logging;

    use super::*;

    #[tokio::test]
    async fn test_next_service_check() {
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

        let next_check = get_next_service_check(&db).await.unwrap();
        dbg!(&next_check);
        assert!(next_check.is_some());
    }
}
