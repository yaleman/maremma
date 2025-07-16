use sea_orm::entity::prelude::*;
use tower_sessions::session::{Id, Record};
use tower_sessions::SessionStore;

use crate::constants::SESSION_EXPIRY_WINDOW_HOURS;
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
    db: Arc<RwLock<DatabaseConnection>>,
}

impl ModelStore {
    pub async fn get_db_lock(
        &self,
    ) -> tokio::sync::RwLockWriteGuard<'_, sea_orm::DatabaseConnection> {
        self.db.write().await
    }
}

fn id_to_uuid(input: &Id) -> Result<Uuid, Error> {
    if input.0 <= 0 {
        return Err(Error::InvalidInput(format!(
            "Input value {} can't be lower than or equal to 0",
            input.0
        )));
    }
    Ok(Uuid::from_u128(input.0 as u128))
}

#[test]
fn test_to_uuid() {
    let id = Id(1);
    let uuid = id_to_uuid(&id).expect("Failed to convert id to uuid");
    assert_eq!(uuid, Uuid::from_u128(1));

    let big_id = Id(u128::MAX as i128 + 1);
    let big_uuid = id_to_uuid(&big_id);
    dbg!(&big_uuid);
    assert!(big_uuid.is_err());
}

#[async_trait]
impl SessionStore for ModelStore {
    #[instrument(level = "debug", skip(self))]
    async fn create(
        &self,
        record: &mut Record,
    ) -> Result<(), tower_sessions::session_store::Error> {
        while record.id.0 <= 0 {
            record.id = Id(rand::random());
        }

        // now we do the database-side things
        let mut dbrecord = ActiveModel::new();
        let id_uuid = id_to_uuid(&record.id)
            .map_err(|err| tower_sessions::session_store::Error::Encode(format!("{err:?}")))?;
        dbrecord.id.set_if_not_equals(id_uuid);
        dbrecord.data.set_if_not_equals(
            serde_json::to_value(&record.data)
                .map_err(|err| tower_sessions::session_store::Error::Encode(err.to_string()))?,
        );
        let chrono_expiry = record.expiry_date.unix_timestamp_nanos();
        let expiry = chrono::DateTime::from_timestamp_nanos(chrono_expiry as i64);

        dbrecord.expiry.set_if_not_equals(expiry);
        dbrecord
            .insert(&*self.db.write().await)
            .await
            .map_err(|err| tower_sessions::session_store::Error::Backend(err.to_string()))?;
        debug!("Created session with id={} uuid={}", record.id.0, id_uuid);
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    async fn save(
        &self,
        session_record: &Record,
    ) -> Result<(), tower_sessions::session_store::Error> {
        let data: Json = serde_json::to_value(&session_record.data)
            .map_err(|err| tower_sessions::session_store::Error::Encode(err.to_string()))?;

        let id = id_to_uuid(&session_record.id)
            .map_err(|err| tower_sessions::session_store::Error::Encode(format!("{err:?}")))?;

        let db_lock = self.get_db_lock().await;

        let mut session = match Entity::find_by_id(id)
            .one(&*db_lock)
            .await
            .map_err(|err| tower_sessions::session_store::Error::Backend(err.to_string()))?
        {
            Some(session) => session,
            None => {
                debug!(
                    "Record not found in the backend for id={} when trying to save",
                    id
                );
                return Err(tower_sessions::session_store::Error::Backend(
                    "Record not found in the backend!".to_string(),
                ));
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
                .update(&*db_lock)
                .await
                .map_err(|err| tower_sessions::session_store::Error::Backend(err.to_string()))?;
            debug!("Saved session with id={}", session_record.id.0);
        } else {
            info!("No changes to save for session id={}", session_record.id.0);
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    async fn load(
        &self,
        session_id: &Id,
    ) -> Result<Option<Record>, tower_sessions::session_store::Error> {
        let id = id_to_uuid(session_id)
            .map_err(|err| tower_sessions::session_store::Error::Decode(format!("{err:?}")))?;
        let db_lock = self.get_db_lock().await;
        let session = match Entity::find_by_id(id)
            .one(&*db_lock)
            .await
            .map_err(|err| tower_sessions::session_store::Error::Backend(err.to_string()))?
        {
            Some(session) => session,
            None => {
                debug!("No session found for id {}", session_id.0);
                return Ok(None);
            }
        };
        drop(db_lock);

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
        Entity::delete_by_id(
            id_to_uuid(session_id).map_err(|err| {
                tower_sessions::session_store::Error::Encode(format!("{err:?}"))
            })?,
        )
        .exec(&*self.db.write().await)
        .await
        .map_err(|err| tower_sessions::session_store::Error::Backend(err.to_string()))?;
        Ok(())
    }
}

impl ModelStore {
    pub fn new(db: Arc<RwLock<DatabaseConnection>>) -> Self {
        Self { db }
    }

    /// Cleans up old/expired sessions
    pub async fn cleanup(&self, db: Arc<RwLock<DatabaseConnection>>) -> Result<u64, Error> {
        let res = Entity::delete_many()
            .filter(
                Column::Expiry
                    .lt(chrono::Utc::now() - chrono::Duration::hours(SESSION_EXPIRY_WINDOW_HOURS)),
            )
            .exec(&*db.write().await)
            .await?;
        Ok(res.rows_affected)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use sea_orm::{ActiveModelTrait, IntoActiveModel};
    use time::Duration;
    use tower_sessions::SessionStore;

    #[tokio::test]
    async fn test_cleanup() {
        let (db, _config) = crate::db::tests::test_setup()
            .await
            .expect("Failed to set up maremma test db");

        let store = crate::db::entities::session::ModelStore { db: db.clone() };

        let session = crate::db::entities::session::Model {
            id: uuid::Uuid::new_v4(),
            expiry: chrono::Utc::now() - chrono::Duration::hours(10),
            data: serde_json::json!({}),
        };

        session
            .into_active_model()
            .insert(&*db.write().await)
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

    #[tokio::test]
    async fn test_lifecycle() {
        let (db, _config) = crate::db::tests::test_setup()
            .await
            .expect("Failed to set up maremma test db");

        let store = crate::db::entities::session::ModelStore::new(db.clone());

        let id = tower_sessions::session::Id(1);

        store
            .create(&mut tower_sessions::session::Record {
                id,
                expiry_date: time::OffsetDateTime::now_utc() + Duration::minutes(100),
                data: HashMap::new(),
            })
            .await
            .expect("Failed to create session");

        let loaded = store.load(&id).await;

        let mut session = loaded
            .expect("Failed to get session")
            .expect("Failed to find session");

        session
            .data
            .insert("hello".to_string(), serde_json::json! {"world"});
        store.save(&session).await.expect("Failed to save session");

        store.delete(&id).await.expect("Failed to delete!")
    }
}
