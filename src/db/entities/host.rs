#![allow(unused_imports)]

use crate::prelude::*;
use sea_orm::entity::prelude::*;
use sea_orm::{ColIdx, IntoActiveModel, QuerySelect, QueryTrait, Set};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "host")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub name: String,
    pub hostname: String,
    pub check: crate::host::HostCheck,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::host_group::Entity")]
    HostGroup,
}

impl Related<super::service::Entity> for Entity {
    fn to() -> RelationDef {
        super::service_check::Relation::Service.def()
    }

    fn via() -> Option<RelationDef> {
        Some(super::service_check::Relation::Service.def().rev())
    }
}

impl Related<super::service_check::Entity> for Entity {
    fn to() -> RelationDef {
        super::service_check::Relation::Host.def()
    }
}

impl Related<super::host_group::Entity> for Entity {
    fn to() -> RelationDef {
        super::host_group::Relation::Host.def()
    }

    fn via() -> Option<RelationDef> {
        Some(super::host_group::Relation::Host.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}

pub async fn find_by_name(name: &str, db: &DatabaseConnection) -> Result<Option<Model>, Error> {
    match Entity::find().filter(Column::Name.eq(name)).one(db).await {
        Ok(val) => Ok(val.into_iter().next()),
        Err(DbErr::RecordNotFound(_)) => Ok(None),
        Err(err) => {
            error!("Query failed while looking up host '{}': {:?}", name, err);
            Err(err.into())
        }
    }
}

#[async_trait]
impl MaremmaEntity for Model {
    async fn update_db_from_config(
        db: Arc<DatabaseConnection>,
        config: &Configuration,
    ) -> Result<(), Error> {
        for (name, host) in config.hosts.clone().into_iter() {
            let model = match find_by_name(&name, db.as_ref()).await {
                Ok(val) => val,
                Err(err) => {
                    error!("Failed to find host '{}': {:?}", name, err);
                    return Err(err);
                }
            };

            match model {
                Some(val) => {
                    debug!("Found host '{:?}'", name);
                    let hostname = match host.hostname {
                        None => name.clone(),
                        Some(val) => val,
                    };

                    let mut existing_host = val.into_active_model();

                    existing_host.check.set_if_not_equals(host.check);
                    existing_host
                        .hostname
                        .set_if_not_equals(hostname.to_owned());
                    existing_host.name.set_if_not_equals(name.to_owned());

                    if existing_host.is_changed() {
                        warn!("Updating {:?}", &existing_host);
                        existing_host.save(db.as_ref()).await?;
                    } else {
                        debug!("No changes to {:?}", &existing_host);
                    }
                }
                None => {
                    let new_host = Model {
                        id: host.id.unwrap_or(Uuid::new_v4()),
                        name: name.to_owned(),
                        hostname: host.hostname.clone().unwrap_or(name.to_string()),
                        check: host.check.clone(),
                    }
                    .into_active_model();
                    warn!("Creating Host {:?}", new_host.insert(db.as_ref()).await?);
                }
            };
        }
        Ok(())
    }
}

// #[cfg(test)]
pub fn test_host() -> Model {
    Model {
        id: Uuid::new_v4(),
        name: "test_host_name".to_string(),
        hostname: "test_host_hostname".to_string(),
        check: crate::host::HostCheck::Ping,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use sea_orm::IntoActiveModel;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use tracing::info;

    use crate::db::entities::MaremmaEntity;
    use crate::setup_logging;

    #[tokio::test]
    async fn test_host_entity() {
        let _ = setup_logging(true);

        let db = crate::db::test_connect()
            .await
            .expect("Failed to connect to database");

        let host = super::test_host();
        info!("saving host...");
        let am = host.clone().into_active_model();
        super::Entity::insert(am).exec(&db).await.unwrap();

        let new_host = super::Entity::find()
            .filter(super::Column::Id.eq(host.id))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        info!("found it: {:?}", new_host);

        super::Entity::delete_by_id(new_host.id)
            .exec(&db)
            .await
            .unwrap();

        assert!(super::Entity::find()
            .filter(super::Column::Id.eq(new_host.id))
            .one(&db)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_update_db_from_config() {
        let db = Arc::new(
            crate::db::test_connect()
                .await
                .expect("Failed to connect to database"),
        );

        let configuration =
            crate::config::Configuration::new(Some(PathBuf::from("maremma.example.json")))
                .await
                .expect("Failed to load configuration");

        super::Model::update_db_from_config(db, &configuration)
            .await
            .expect("Failed to load config");
    }
    #[tokio::test]
    async fn test_create_then_search() {
        let _ = setup_logging(true);

        let db = Arc::new(
            crate::db::test_connect()
                .await
                .expect("Failed to connect to database"),
        );

        let inserted_host = super::Entity::insert(super::test_host().into_active_model())
            .exec_with_returning(db.as_ref())
            .await
            .expect("Failed to insert host");

        let found_host = super::find_by_name(&super::test_host().name, db.as_ref())
            .await
            .expect("Failed to query host");

        assert!(found_host.is_some());

        assert_eq!(found_host.unwrap().name, inserted_host.name);
    }
}
