use entities::service_check;

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
        _db: &DatabaseConnection,
        _service_check_id: Uuid,
        _count: u64,
    ) -> Result<(), Error> {
        // TODO: implement this
        todo!()
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
}
