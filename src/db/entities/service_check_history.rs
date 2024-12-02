use entities::service_check;
use sea_orm::QueryOrder;

use crate::prelude::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "service_check_history")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub service_check_id: Uuid,
    pub status: ServiceStatus,
    pub time_elapsed: i64,
    pub result_text: String,
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    ServiceCheck,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::ServiceCheck => Entity::belongs_to(service_check::Entity)
                .from(Column::ServiceCheckId)
                .to(service_check::Column::Id)
                .into(),
        }
    }
}

impl Related<service_check::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ServiceCheck.def()
    }
}

impl Entity {
    /// Leaves only the last x number of service check history entries
    pub async fn head(
        db: &DatabaseConnection,
        service_check_id: Option<Uuid>,
        leave_remaining: u64,
    ) -> Result<usize, Error> {
        let mut trimmed = 0;

        // find all the service checks
        let mut service_checks = entities::service_check::Entity::find();
        if let Some(sc) = service_check_id {
            service_checks = service_checks.filter(entities::service_check::Column::Id.eq(sc));
        }
        let service_checks = service_checks.all(db).await?;

        if service_checks.len() > 1 && service_check_id.is_some() {
            Err(Error::Generic(
                "More than one service check found when asking for a single!".to_string(),
            ))?;
        }

        for service_check in &service_checks {
            let to_delete = service_check
                .find_related(entities::service_check_history::Entity)
                .order_by_desc(Column::Timestamp)
                .all(db)
                .await?;

            // let mut num_records: usize = to_delete.len();
            if to_delete.is_empty() {
                debug!(
                    "No service check history for service check id {}",
                    service_check.id
                );
                // num_records = 0;
            } else {
                let offset_list: Vec<Uuid> = to_delete
                    .into_iter()
                    .skip(leave_remaining as usize)
                    .map(|x| x.id)
                    .collect();
                debug!("Deleting {} records", offset_list.len());

                trimmed += offset_list.len();
                Entity::delete_many()
                    .filter(Column::ServiceCheckId.is_in(offset_list))
                    .exec(db)
                    .await?;
            }
        }
        info!(
            "Removed {} records across {} service checks",
            trimmed,
            service_checks.len()
        );
        Ok(trimmed)
    }

    /// Prunes the service check history table
    pub async fn prune(
        db: &DatabaseConnection,
        after_time: DateTime<Utc>,
        service_check_id: Option<Uuid>,
    ) -> Result<u64, Error> {
        if after_time > Utc::now() {
            return Err(Error::DateIsInTheFuture);
        }
        let mut query = Entity::delete_many().filter(Column::Timestamp.lt(after_time));
        if let Some(service_check_id) = service_check_id {
            query = query.filter(Column::ServiceCheckId.eq(service_check_id));
        }

        let res = query.exec(db).await?;

        Ok(res.rows_affected)
    }
}

impl Model {
    pub fn from_service_check_result(service_check_id: Uuid, result: &CheckResult) -> Self {
        Self {
            id: Uuid::new_v4(),
            service_check_id,
            status: result.status,
            timestamp: Utc::now(),
            time_elapsed: result.time_elapsed.num_milliseconds(),
            result_text: result.result_text.clone(),
        }
    }
}

impl ActiveModelBehavior for ActiveModel {}

#[cfg(test)]
mod tests {

    use crate::db::tests::test_setup;

    use super::*;

    #[tokio::test]
    async fn test_service_check_history() {
        let (db, _config) = test_setup().await.expect("Failed to do test setup");

        let service_check = entities::service_check::Entity::find()
            .one(db.as_ref())
            .await
            .expect("Failed to query service check")
            .expect("Failed to find service check");

        let result = CheckResult {
            timestamp: Utc::now(),
            time_elapsed: chrono::Duration::milliseconds(145),
            status: ServiceStatus::Ok,
            result_text: "test".to_string(),
        };
        let service_check_history = Model::from_service_check_result(service_check.id, &result);

        let res = service_check_history
            .clone()
            .into_active_model()
            .insert(db.as_ref())
            .await
            .expect("Failed to save service check history");

        assert!(res.id != Uuid::nil());

        let res = Entity::find_by_id(service_check_history.id)
            .find_with_related(entities::service_check::Entity)
            .all(db.as_ref())
            .await
            .expect("Failed to find service check history");

        let (model, related_model) = res.first().expect("Failed to get first result");
        assert_eq!(model.id, service_check_history.id);
        assert!(!related_model.is_empty());

        let res = Entity::prune(
            db.as_ref(),
            chrono::Utc::now() - TimeDelta::days(1),
            Some(service_check.id),
        )
        .await
        .expect("Failed to prune service check history");

        assert_eq!(res, 0);
    }

    #[tokio::test]
    async fn test_future_date_prune() {
        let (db, _config) = test_setup().await.expect("Failed to do test setup");

        let res = Entity::prune(db.as_ref(), chrono::Utc::now() + TimeDelta::days(1), None).await;

        assert!(matches!(res, Err(Error::DateIsInTheFuture)));
    }
    #[tokio::test]
    async fn test_prune_service_check_id() {
        let (db, _config) = test_setup().await.expect("Failed to do test setup");

        let res = Entity::prune(
            db.as_ref(),
            chrono::Utc::now() - TimeDelta::days(1),
            Some(Uuid::new_v4()),
        )
        .await
        .expect("Failed to prune nothing");

        assert_eq!(res, 0);
    }
    #[tokio::test]
    async fn test_head_service_check_history() {
        let (db, _config) = test_setup().await.expect("Failed to do test setup");

        let res = Entity::head(db.as_ref(), Some(Uuid::new_v4()), 0)
            .await
            .expect("Failed to prune nothing");

        assert_eq!(res, 0);
    }
    #[tokio::test]
    async fn test_head_service_check_history_sc_id() {
        let (db, _config) = test_setup().await.expect("Failed to do test setup");

        let valid_service_check = entities::service_check::Entity::find()
            .one(db.as_ref())
            .await
            .expect("Failed to find service check")
            .expect("Failed to find service check");

        let result = CheckResult {
            timestamp: Utc::now(),
            time_elapsed: chrono::Duration::milliseconds(145),
            status: ServiceStatus::Ok,
            result_text: "test".to_string(),
        };
        let service_check_history =
            Model::from_service_check_result(valid_service_check.id, &result);

        service_check_history
            .clone()
            .into_active_model()
            .insert(db.as_ref())
            .await
            .expect("Failed to save service check history");

        let res = Entity::head(db.as_ref(), Some(valid_service_check.id), 0)
            .await
            .expect("Failed to prune a valid SCID");

        assert_eq!(res, 1);

        let res = Entity::head(db.as_ref(), Some(Uuid::new_v4()), 0)
            .await
            .expect("Failed to prune nothing");

        assert_eq!(res, 0);
    }

    #[tokio::test]
    async fn test_head_1k() {
        let (db, _config) = test_setup().await.expect("Failed to do test setup");

        let valid_service_check = entities::service_check::Entity::find()
            .one(db.as_ref())
            .await
            .expect("Failed to find service check")
            .expect("Failed to find service check");

        let valid_sc_id = valid_service_check.id.to_owned();

        let result = CheckResult {
            timestamp: Utc::now(),
            time_elapsed: chrono::Duration::milliseconds(145),
            status: ServiceStatus::Ok,
            result_text: "test".to_string(),
        };

        let things_to_create: u64 = 50;
        let num_to_delete = 10;

        for _ in 0..things_to_create {
            let mut sch =
                Model::from_service_check_result(valid_sc_id, &result).into_active_model();

            sch.id.set_if_not_equals(Uuid::new_v4());
            sch.insert(db.as_ref())
                .await
                .expect("Failed to save service check history");
        }

        let res = Entity::find()
            .filter(Column::ServiceCheckId.eq(valid_sc_id))
            .all(db.as_ref())
            .await
            .expect("Failed to find service check history");
        assert_eq!(res.len(), things_to_create as usize);
        info!(
            "Have {} records for Service check id {}",
            res.len(),
            valid_sc_id
        );

        let res = Entity::head(db.as_ref(), Some(valid_service_check.id), num_to_delete)
            .await
            .expect("Failed to prune nothing");

        assert_eq!(res, (things_to_create - num_to_delete) as usize);
    }
}
