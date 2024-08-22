use sea_orm::entity::prelude::*;
use tower_sessions::session::{Id, Record};
use tower_sessions::SessionStore;

use crate::constants::{SESSION_EXPIRY_DEFAULT_MINUTES, SESSION_EXPIRY_WINDOW_HOURS};
use crate::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "session")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, name = "id")]
    pub id: Uuid,
    pub expiry: chrono::DateTime<Utc>,
    pub data: Json,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

#[derive(Debug, Clone)]
pub struct ModelStore {
    db: Arc<DatabaseConnection>,
    session_length_minutes: Option<u32>,
}

#[async_trait]
impl SessionStore for ModelStore {
    #[instrument(level = "debug", skip(self))]
    async fn create(
        &self,
        record: &mut Record,
    ) -> Result<(), tower_sessions::session_store::Error> {
        let id = Uuid::new_v4();
        let expiry = chrono::Utc::now()
            + Duration::minutes(
                self.session_length_minutes
                    .unwrap_or(SESSION_EXPIRY_DEFAULT_MINUTES) as i64,
            );

        record.id = Id(id.as_u128() as i128);

        // if the timestamp nanos overflows then you've been real weird.
        record.expiry_date = time::OffsetDateTime::from_unix_timestamp_nanos(
            expiry.timestamp_nanos_opt().unwrap_or(0).into(),
        )
        .map_err(|err| tower_sessions::session_store::Error::Encode(err.to_string()))?;

        // now we do the database-side things
        let mut dbrecord = ActiveModel::new();
        dbrecord.id.set_if_not_equals(id);
        dbrecord.data.set_if_not_equals(
            serde_json::to_value(&record.data)
                .map_err(|err| tower_sessions::session_store::Error::Encode(err.to_string()))?,
        );
        dbrecord.expiry.set_if_not_equals(expiry);
        dbrecord
            .insert(self.db.as_ref())
            .await
            .map_err(|err| tower_sessions::session_store::Error::Backend(err.to_string()))?;
        // done!
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    async fn save(
        &self,
        session_record: &Record,
    ) -> Result<(), tower_sessions::session_store::Error> {
        let data: Json = serde_json::to_value(&session_record.data)
            .map_err(|err| tower_sessions::session_store::Error::Encode(err.to_string()))?;

        let id = Uuid::from_u128(session_record.id.0 as u128);

        let mut session = match Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await
            .map_err(|err| tower_sessions::session_store::Error::Backend(err.to_string()))?
        {
            Some(session) => session,
            None => {
                return Err(tower_sessions::session_store::Error::Backend(
                    "Record not found in the backend!".to_string(),
                ))
            }
        }
        .into_active_model();

        let expiry = chrono::DateTime::from_timestamp_nanos(
            session_record.expiry_date.unix_timestamp_nanos() as i64,
        );
        session.id.set_if_not_equals(id);
        session.expiry.set_if_not_equals(expiry);
        session.data.set_if_not_equals(data);

        if session.is_changed() {
            session
                .save(self.db.as_ref())
                .await
                .map_err(|err| tower_sessions::session_store::Error::Backend(err.to_string()))?;
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    async fn load(
        &self,
        session_id: &Id,
    ) -> Result<Option<Record>, tower_sessions::session_store::Error> {
        let id = Uuid::from_u128(session_id.0 as u128);
        let session = match Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await
            .map_err(|err| tower_sessions::session_store::Error::Backend(err.to_string()))?
        {
            Some(session) => session,
            None => return Ok(None),
        };

        let id = session.id.as_u128();

        let session_expiry_nanos = session.expiry.timestamp_nanos_opt().ok_or(
            tower_sessions::session_store::Error::Encode("Failed to encode timestamp".to_string()),
        )? as i128;

        let expiry_date = time::OffsetDateTime::from_unix_timestamp_nanos(session_expiry_nanos)
            .map_err(|err| tower_sessions::session_store::Error::Backend(err.to_string()))?;

        Ok(Some(Record {
            id: Id(id as i128),
            expiry_date,
            data: serde_json::from_value(session.data.clone())
                .map_err(|err| tower_sessions::session_store::Error::Decode(err.to_string()))?,
        }))
    }

    #[instrument(level = "debug", skip(self))]
    async fn delete(&self, session_id: &Id) -> Result<(), tower_sessions::session_store::Error> {
        let id = Uuid::from_u128(session_id.0 as u128);

        Entity::delete_by_id(id)
            .exec(self.db.as_ref())
            .await
            .map_err(|err| tower_sessions::session_store::Error::Backend(err.to_string()))?;
        Ok(())
    }
}

impl ModelStore {
    #[must_use]
    pub fn new(db: Arc<DatabaseConnection>, session_length_minutes: Option<u32>) -> Self {
        Self {
            db,
            session_length_minutes,
        }
    }

    /// Cleans up old/expired sessions
    pub async fn cleanup(&self, db: Arc<DatabaseConnection>) -> Result<u64, Error> {
        let res = Entity::delete_many()
            .filter(
                Column::Expiry
                    .lt(chrono::Utc::now() - chrono::Duration::hours(SESSION_EXPIRY_WINDOW_HOURS)),
            )
            .exec(db.as_ref())
            .await?;
        Ok(res.rows_affected)
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ActiveModelTrait, IntoActiveModel};

    #[tokio::test]
    async fn test_cleanup() {
        let (db, config) = crate::db::tests::test_setup()
            .await
            .expect("Failed to set up maremma test db");

        let store = crate::db::entities::session::ModelStore {
            db: db.clone(),
            session_length_minutes: config.web_session_length_minutes,
        };

        let session = crate::db::entities::session::Model {
            id: uuid::Uuid::new_v4(),
            expiry: chrono::Utc::now() - chrono::Duration::hours(10),
            data: serde_json::json!({}),
        };

        session
            .into_active_model()
            .insert(db.as_ref())
            .await
            .expect("Failed to insert test session!");

        let res = store
            .cleanup(db.clone())
            .await
            .expect("Failed to cleanup sessions");
        assert_eq!(res, 1);

        let res = store
            .cleanup(db.clone())
            .await
            .expect("Failed to cleanup sessions");
        assert_eq!(res, 0);
    }
}
