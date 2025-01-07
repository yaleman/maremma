//! Cleans up old service check history entries so we don't end up with a database the size of a smol planet

use super::prelude::*;

pub(crate) struct ServiceCheckHistoryCleanerTask {
    config: SendableConfig,
}

impl ServiceCheckHistoryCleanerTask {
    pub(crate) fn new(config: SendableConfig) -> Self {
        Self { config }
    }
}

#[derive(Debug, FromQueryResult)]
struct SimpleSchCounts {
    #[allow(dead_code)]
    pub service_check_id: Uuid,
    #[allow(dead_code)]
    pub count: i64,
}

#[async_trait]
impl CronTaskTrait for ServiceCheckHistoryCleanerTask {
    async fn run(&mut self, db: &DatabaseConnection) -> Result<(), Error> {
        let sch_counts: Vec<SimpleSchCounts> = entities::service_check_history::Entity::find()
            .column(entities::service_check_history::Column::ServiceCheckId)
            .column_as(
                entities::service_check_history::Column::ServiceCheckId.count(),
                "count",
            )
            .group_by(entities::service_check_history::Column::ServiceCheckId)
            .order_by(
                entities::service_check_history::Column::ServiceCheckId.count(),
                Order::Desc,
            )
            .limit(10) // if we only clean up a few at a time it's less likely to cause a huge spike in db contention
            .into_model::<SimpleSchCounts>()
            .all(db)
            .await
            .inspect_err(|err| error!("Service check history cleaner failed: {:?}", err))?;
        debug!("sch counts: {:?}", sch_counts);

        let sch_counts = sch_counts
            .into_iter()
            .map(|x| (x.service_check_id, x.count))
            .collect::<Vec<(_, _)>>();

        let target_num = self.config.read().await.max_history_entries_per_check;

        for (id, count) in sch_counts {
            if count as u64 <= target_num {
                debug!(
                    "Service check {} only has {} entries, less than {}, skipping",
                    id, target_num, count
                );
                continue;
            }
            if let Some(target_service_check) = entities::service_check::Entity::find_by_id(id)
                .one(db)
                .await?
            {
                let res = entities::service_check_history::Entity::head(
                    db,
                    Some(target_service_check.id),
                    target_num,
                )
                .await?;
                info!(
                    "Deleted {} old service check history entries for {}",
                    res, target_service_check.id
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::db::tests::test_setup_quieter;
    use entities::service_check_history;
    use sea_orm::{ActiveModelTrait, Set};
    use uuid::Uuid;

    use super::*;

    #[tokio::test]
    async fn test_service_check_history_cleaner() {
        let (db, config) = test_setup_quieter().await.expect("Failed to do test setup");
        config.write().await.max_history_entries_per_check = 1;

        let valid_service_check = entities::service_check::Entity::find()
            .one(db.as_ref())
            .await
            .expect("Failed to find service check")
            .expect("Failed to find service check");

        let max = 35000;
        info!("Creating {} service check history entries", max);

        for _ in 0..max {
            service_check_history::ActiveModel {
                id: Set(Uuid::new_v4()),
                service_check_id: Set(valid_service_check.id),
                timestamp: Set(chrono::Utc::now()),
                status: Set(ServiceStatus::Ok),
                result_text: Set(valid_service_check.id.to_string()),
                time_elapsed: Set(0 as i64),
                ..Default::default()
            }
            .insert(&*db)
            .await
            .expect("Failed to insert service check history for check 1");
        }

        let mut task = ServiceCheckHistoryCleanerTask::new(config);
        task.run(&*db).await.expect("Failed to run task");
    }
}
