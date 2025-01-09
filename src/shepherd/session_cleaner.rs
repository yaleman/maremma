//! Keeps track of old sessions
//!

use super::prelude::*;
pub(crate) struct SessionCleanTask {}

#[async_trait]
impl CronTaskTrait for SessionCleanTask {
    async fn run(&mut self, db: Arc<RwLock<DatabaseConnection>>) -> Result<(), Error> {
        debug!("Checking sessions for cleanup...");

        let res = entities::session::Entity::delete_many()
            .filter(
                entities::session::Column::Expiry
                    .lt(Utc::now() - chrono::Duration::hours(SESSION_EXPIRY_WINDOW_HOURS)),
            )
            .exec(&*db.write().await)
            .await
            .inspect_err(|err| error!("Session cleaner failed: {:?}", err))?;
        if res.rows_affected == 0 {
            debug!("No old sessions found.");
        } else {
            info!("Cleared {} expired sessions.", res.rows_affected);
        }
        Ok(())
    }
}
