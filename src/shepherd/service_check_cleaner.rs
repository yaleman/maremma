//! Cleans up old service check histories

use super::prelude::*;

pub(crate) struct ServiceCheckCleanTask {}

#[async_trait]
impl CronTaskTrait for ServiceCheckCleanTask {
    async fn run(&mut self, db: Arc<RwLock<DatabaseConnection>>) -> Result<(), Error> {
        debug!("Checking for stuck service checks...");

        let res: sea_orm::UpdateResult = entities::service_check::Entity::update_many()
            .col_expr(
                entities::service_check::Column::Status,
                Expr::value(ServiceStatus::Pending),
            )
            .filter(
                entities::service_check::Column::Status
                    .eq(ServiceStatus::Checking)
                    .and(
                        entities::service_check::Column::LastUpdated
                            .lt(Utc::now() - chrono::Duration::minutes(STUCK_CHECK_MINUTES)),
                    ),
            )
            .exec(&*db.write().await)
            .await?;

        if res.rows_affected == 0 {
            debug!("No stuck service checks found.");
        } else {
            info!("Reset {} stuck service checks.", res.rows_affected);
        }
        Ok(())
    }
}
