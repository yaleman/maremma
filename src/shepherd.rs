//! The shepherd wanders around making sure things are in order.

use std::sync::Arc;

use axum::async_trait;
use chrono::{DateTime, Utc};
use croner::Cron;
use sea_orm::prelude::Expr;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use tracing::{debug, info};

use crate::config::Configuration;
use crate::constants::{SESSION_EXPIRY_WINDOW_HOURS, STUCK_CHECK_MINUTES};
use crate::db::entities;
use crate::db::entities::service_check::Column;
use crate::errors::Error;
use crate::prelude::ServiceStatus;

struct CronTask {
    cron: Cron,
    last_run: DateTime<Utc>,
    task: Box<dyn CronTaskTrait>,
}

impl CronTask {
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

/// The shepherd wanders around making sure things are in order.
pub async fn shepherd(
    db: Arc<DatabaseConnection>,
    _config: Arc<Configuration>,
) -> Result<(), Error> {
    // run the clean_up_checking loop every x minutes
    let mut service_check_clean = CronTask {
        cron: Cron::new("*/1 * * * *").parse()?,
        last_run: Utc::now(),
        task: Box::new(ServiceCheckCleanTask {}),
    };

    // run the session clean up check every hour
    let mut session_cleaner = CronTask {
        cron: Cron::new("10 1 * * *").parse()?,
        last_run: Utc::now(),
        task: Box::new(SessionCleanTask {}),
    };

    loop {
        debug!("The shepherd is checking the herd...");

        if service_check_clean.should_run()? {
            service_check_clean.task.run(db.as_ref()).await?;
            service_check_clean.has_run();
        }
        if session_cleaner.should_run()? {
            session_cleaner.task.run(db.as_ref()).await?;
            session_cleaner.has_run();
        }
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

        let res = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            super::shepherd(db, config),
        )
        .await;

        eprintln!("{:?}", res);
    }
}
