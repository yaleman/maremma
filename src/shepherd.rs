//! The shepherd wanders around making sure things are in order.

use std::sync::Arc;

use axum::async_trait;
use chrono::{DateTime, Utc};
use croner::Cron;
use sea_orm::prelude::Expr;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use tracing::{debug, error, info};

use crate::config::Configuration;
use crate::constants::{SESSION_EXPIRY_WINDOW_HOURS, STUCK_CHECK_MINUTES};
use crate::db::entities;
use crate::db::entities::service_check::Column;
use crate::errors::Error;
use crate::prelude::ServiceStatus;
use crate::web::controller::WebServerControl;

struct CronTask {
    cron: Cron,
    last_run: DateTime<Utc>,
    task: Box<dyn CronTaskTrait>,
}

impl CronTask {
    async fn run_task(&mut self, db: &DatabaseConnection) -> Result<bool, Error> {
        if self.should_run()? {
            self.task.run(db).await?;
            self.has_run();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn should_run(&self) -> Result<bool, Error> {
        let next_occurrence = self.cron.find_next_occurrence(&self.last_run, false)?;
        Ok(next_occurrence <= chrono::Utc::now())
    }

    fn has_run(&mut self) {
        self.last_run = Utc::now();
    }
}

#[async_trait]
trait CronTaskTrait {
    async fn run(&mut self, db: &DatabaseConnection) -> Result<(), Error>;
}

struct ServiceCheckCleanTask {}

#[async_trait]
impl CronTaskTrait for ServiceCheckCleanTask {
    async fn run(&mut self, db: &DatabaseConnection) -> Result<(), Error> {
        debug!("Checking for stuck service checks...");

        let res = entities::service_check::Entity::update_many()
            .col_expr(Column::Status, Expr::value(ServiceStatus::Pending))
            .filter(Column::Status.eq(ServiceStatus::Checking).and(
                Column::LastUpdated.lt(Utc::now() - chrono::Duration::minutes(STUCK_CHECK_MINUTES)),
            ))
            .exec(db)
            .await?;

        if res.rows_affected == 0 {
            debug!("No stuck service checks found.");
        } else {
            info!("Reset {} stuck service checks.", res.rows_affected);
        }
        Ok(())
    }
}
struct SessionCleanTask {}
#[async_trait]
impl CronTaskTrait for SessionCleanTask {
    async fn run(&mut self, db: &DatabaseConnection) -> Result<(), Error> {
        debug!("Checking sessions for cleanup...");

        let res = entities::session::Entity::delete_many()
            .filter(
                entities::session::Column::Expiry
                    .lt(Utc::now() - chrono::Duration::hours(SESSION_EXPIRY_WINDOW_HOURS)),
            )
            .exec(db)
            .await?;
        if res.rows_affected == 0 {
            debug!("No old sessions found.");
        } else {
            info!("Cleared {} expired sessions.", res.rows_affected);
        }
        Ok(())
    }
}

/// Task to check if any certificates have changed
struct CertReloaderTask {
    tx: tokio::sync::mpsc::Sender<WebServerControl>,
    config: Arc<Configuration>,
    cert_time: DateTime<Utc>,
    key_time: DateTime<Utc>,
}

/// Get the last modified time of a file
fn get_file_time(file: &std::path::Path) -> Result<DateTime<Utc>, Error> {
    let file = file.canonicalize().inspect_err(|err| {
        error!(
            "Failed to get canonical path for {} error={:?}",
            file.display(),
            err
        )
    })?;

    let metadata = file.metadata()?;
    let modified = metadata.modified()?;
    Ok(DateTime::<Utc>::from(modified))
}

fn get_file_times(config: &Configuration) -> Result<(DateTime<Utc>, DateTime<Utc>), Error> {
    let cert_time = get_file_time(&config.cert_file).inspect_err(|err| {
        error!(
            "Failed to get metadata for TLS cert at {} {:?}",
            config.cert_file.display(),
            err
        )
    })?;
    let key_time = get_file_time(&config.cert_key).inspect_err(|err| {
        error!(
            "Failed to get metadata for TLS key at {} {:?}",
            config.cert_key.display(),
            err
        )
    })?;
    Ok((cert_time, key_time))
}

impl CertReloaderTask {
    async fn new(
        tx: tokio::sync::mpsc::Sender<WebServerControl>,
        config: Arc<Configuration>,
    ) -> Result<Self, Error> {
        // get the time for the cert
        if !config.cert_file.exists() {
            return Err(Error::Configuration(format!(
                "Couldn't find cert file at {}",
                config.cert_file.display()
            )));
        }
        if !config.cert_key.exists() {
            return Err(Error::Configuration(format!(
                "Couldn't find cert key file at {}",
                config.cert_key.display()
            )));
        }

        let (cert_time, key_time) = get_file_times(&config)?;

        Ok(Self {
            tx,
            config,
            cert_time,
            key_time,
        })
    }
}

#[async_trait]
impl CronTaskTrait for CertReloaderTask {
    async fn run(&mut self, _db: &DatabaseConnection) -> Result<(), Error> {
        let (cert_time, key_time) = get_file_times(&self.config)?;

        if cert_time != self.cert_time || key_time != self.key_time {
            info!("TLS cert or key has changed, reloading...");
            self.cert_time = cert_time;
            self.key_time = key_time;
            if self.tx.send(WebServerControl::Reload).await.is_err() {
                error!("Tried to tell the web server to reload but couldn't!");
                return Err(Error::IoError(
                    "Tried to tell the web server to reload but couldn't!".to_string(),
                ));
            }
        }
        self.cert_time = cert_time;
        self.key_time = key_time;
        Ok(())
    }
}

/// The shepherd wanders around making sure things are in order.
pub async fn shepherd(
    db: Arc<DatabaseConnection>,
    config: Arc<Configuration>,
    web_tx: tokio::sync::mpsc::Sender<WebServerControl>,
) -> Result<(), Error> {
    // run the clean_up_checking loop every x minutes
    let mut service_check_clean = CronTask {
        cron: Cron::new("* * * * *").parse()?,
        last_run: Utc::now(),
        task: Box::new(ServiceCheckCleanTask {}),
    };

    // run the session clean up check every hour
    let mut session_cleaner = CronTask {
        cron: Cron::new("*/3 * * * *").parse()?,
        last_run: Utc::now(),
        task: Box::new(SessionCleanTask {}),
    };

    let mut check_cert_changed = CronTask {
        cron: Cron::new("* * * * *").parse()?,
        last_run: Utc::now(),
        task: Box::new(CertReloaderTask::new(web_tx, config.clone()).await?),
    };

    loop {
        debug!("The shepherd is checking the herd...");

        service_check_clean.run_task(db.as_ref()).await?;
        session_cleaner.run_task(db.as_ref()).await?;
        check_cert_changed.run_task(db.as_ref()).await?;

        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }
}

#[cfg(test)]
mod tests {
    use croner::Cron;

    use crate::db::tests::test_setup;
    use crate::shepherd::{CronTask, CronTaskTrait, ServiceCheckCleanTask, SessionCleanTask};

    #[tokio::test]
    async fn test_servicecheckcleantask() {
        let (db, _config) = test_setup().await.expect("Failed to set up tests");

        let mut scct = ServiceCheckCleanTask {};
        scct.run(&db)
            .await
            .expect("Failed to run ServiceCheckCleanTask");
    }
    #[tokio::test]
    async fn test_sessioncleantask() {
        let (db, _config) = test_setup().await.expect("Failed to set up tests");

        let mut crontask = CronTask {
            task: Box::new(SessionCleanTask {}),
            cron: Cron::new("* * * * *")
                .parse()
                .expect("Failed to create cron"),
            last_run: chrono::Utc::now(),
        };

        crontask
            .task
            .run(&db)
            .await
            .expect("Failed to run SessionCleanTask");
    }

    #[tokio::test]
    async fn test_shepherd() {
        let (db, config) = test_setup().await.expect("Failed to set up tests");

        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        let res = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            super::shepherd(db, config, tx.clone()),
        )
        .await;

        eprintln!("{:?}", res);
    }
}
