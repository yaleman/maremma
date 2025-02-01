//! The shepherd wanders around making sure things are in order.

mod cert_reloader;
pub(crate) mod prelude;
mod service_check_cleaner;
mod service_check_history_cleaner;
mod session_cleaner;

use cert_reloader::CertReloaderTask;
use prelude::*;
use service_check_cleaner::ServiceCheckCleanTask;
use service_check_history_cleaner::ServiceCheckHistoryCleanerTask;
use session_cleaner::SessionCleanTask;
use tokio::sync::RwLock;

pub(crate) struct CronTask {
    name: String,
    cron: Cron,
    last_run: DateTime<Utc>,
    task: Box<dyn CronTaskTrait>,
}

impl CronTask {
    pub(crate) fn new(name: String, cron: Cron, task: Box<dyn CronTaskTrait>) -> Self {
        Self {
            name,
            cron,
            last_run: Utc::now(),
            task,
        }
    }

    fn with_last_run(self, last_run: DateTime<Utc>) -> Self {
        Self { last_run, ..self }
    }

    #[instrument(level = "INFO", skip_all)]
    async fn run_task(&mut self, db: Arc<RwLock<DatabaseConnection>>) -> Result<bool, Error> {
        if self.should_run()? {
            self.task
                .run(db)
                .await
                .inspect_err(|err| error!("{} task threw an error: {:?}", self.name, err))?;
            self.last_run = Utc::now();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn should_run(&self) -> Result<bool, Error> {
        let next_occurrence = self.cron.find_next_occurrence(&self.last_run, false)?;
        Ok(next_occurrence <= chrono::Utc::now())
    }
}

#[async_trait]
pub(crate) trait CronTaskTrait {
    async fn run(&mut self, db: Arc<RwLock<DatabaseConnection>>) -> Result<(), Error>;
}

/// The shepherd wanders around making sure things are in order.
pub async fn shepherd(
    db: Arc<RwLock<DatabaseConnection>>,
    config: SendableConfig,
    web_tx: tokio::sync::mpsc::Sender<WebServerControl>,
) -> Result<(), Error> {
    // run the clean_up_checking loop every x minutes
    let mut service_check_clean = CronTask::new(
        "ServiceCheckClean".to_string(),
        Cron::new("* * * * *").parse()?,
        Box::new(ServiceCheckCleanTask {}),
    );

    // run the session clean up check every hour
    let mut session_cleaner = CronTask::new(
        "SessionCleaner".to_string(),
        Cron::new("49 * * * *").parse()?,
        Box::new(SessionCleanTask {}),
    );

    let mut check_cert_changed = CronTask::new(
        "CheckCertChanged".to_string(),
        Cron::new("* * * * *").parse()?,
        Box::new(CertReloaderTask::new(web_tx, config.clone()).await?),
    );

    let mut service_check_history_cleaner: CronTask = CronTask::new(
        "ServiceCheckHistoryCleaner".to_string(),
        Cron::new("27 * * * *").parse()?,
        Box::new(ServiceCheckHistoryCleanerTask::new(config.clone())),
    )
    .with_last_run(Utc::now() + Duration::minutes(5));

    loop {
        let start_time = std::time::SystemTime::now();
        debug!("The shepherd is checking the herd...");

        let tasks = vec![
            service_check_clean.run_task(db.clone()),
            session_cleaner.run_task(db.clone()),
            check_cert_changed.run_task(db.clone()),
            service_check_history_cleaner.run_task(db.clone()),
        ];

        futures::future::try_join_all(tasks).await?;

        // work out how long it took and go through to clean up
        let elapsed = start_time
            .elapsed()
            .unwrap_or(std::time::Duration::from_secs(0));

        if elapsed.as_secs() < 60 {
            tokio::time::sleep(std::time::Duration::from_secs(60) - elapsed).await;
        } else {
            warn!("The shepherd is running late, no sleep for them!");
        }
    }
}

#[cfg(test)]
mod tests {
    use croner::Cron;

    use super::*;
    use crate::db::tests::test_setup;

    #[tokio::test]
    async fn test_servicecheckcleantask() {
        let (db, _config) = test_setup().await.expect("Failed to set up tests");

        let mut scct = ServiceCheckCleanTask {};
        scct.run(db)
            .await
            .expect("Failed to run ServiceCheckCleanTask");
    }
    #[tokio::test]
    async fn test_sessioncleantask() {
        let (db, _config) = test_setup().await.expect("Failed to set up tests");

        let mut crontask = CronTask::new(
            "test_task".to_string(),
            Cron::new("* * * * *")
                .parse()
                .expect("Failed to create cron"),
            Box::new(SessionCleanTask {}),
        );

        crontask
            .task
            .run(db)
            .await
            .expect("Failed to run SessionCleanTask");

        assert!(!crontask.should_run().expect("Failed to check should_run"),);
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

        dbg!(&res);
    }
}
