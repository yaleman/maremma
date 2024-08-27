//! Dropping the host_groups column from the Service table

use sea_orm::prelude::Expr;
use sea_orm::sea_query::{self, ColumnDef, Table};
use sea_orm::{ColumnTrait, DbErr, EntityTrait, Iden, QueryFilter};
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use tracing::debug;
use uuid::Uuid;

use crate::db::entities;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20240827_add_host_config_column" // Make sure this matches with the file name
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // Define how to apply this migration: Create the table.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .add_column_if_not_exists(ColumnDef::new(Host::Config).json())
                    .table(Host::Table)
                    .to_owned(),
            )
            .await?;

        // set the maremma-local host to uuid 0 for the host id
        let db = manager.get_connection();

        // we're adding the config column and setting it to null for all hosts
        entities::host::Entity::update_many()
            .col_expr(
                entities::host::Column::Config,
                Expr::value(sea_orm::Value::Json(Some(Box::new(serde_json::json!({}))))),
            )
            .exec(db)
            .await?;

        if entities::host::Entity::find()
            .filter(
                entities::host::Column::Name
                    .eq(crate::LOCAL_SERVICE_HOST_NAME)
                    .and(entities::host::Column::Hostname.eq(crate::LOCAL_SERVICE_HOST_NAME)),
            )
            .one(db)
            .await?
            .is_some()
        {
            debug!("Setting local host to id 0");
            entities::host::Entity::update_many()
                .col_expr(entities::host::Column::Id, Expr::value(Uuid::from_u128(0)))
                .filter(
                    entities::host::Column::Name
                        .eq(crate::LOCAL_SERVICE_HOST_NAME)
                        .and(entities::host::Column::Hostname.eq(crate::LOCAL_SERVICE_HOST_NAME)),
                )
                .exec(db)
                .await?;
        }

        Ok(())
    }

    // Define how to rollback this migration
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .drop_column(Host::Config)
                    .table(Host::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
pub enum Host {
    Table,
    Config,
}
