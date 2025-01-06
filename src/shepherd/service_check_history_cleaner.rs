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
        println!("sch counts: {:?}", sch_counts);

        let target_num = self.config.read().await.max_history_entries_per_check;

        for target_sch in sch_counts {
            if target_sch.count as u64 <= target_num {
                debug!(
                    "Service check {} only has {} entries, less than {}, skipping",
                    target_sch.service_check_id, target_num, target_sch.count
                );
                continue;
            }

            if let Some(target_service_check) =
                entities::service_check::Entity::find_by_id(target_sch.service_check_id)
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
