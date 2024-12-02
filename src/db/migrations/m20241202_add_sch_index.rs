//! Creating an index on the ServiceCheckHistory table so we can filter by service_check_id faster

use super::m20240807_create_service_check_hstory_table::ServiceCheckHistory;
use sea_orm::sea_query::{self};
use sea_orm::DbErr;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20241202_add_sch_index" // Make sure this matches with the file name
    }
}

const INDEX_NAME: &str = "idx_service_check_history";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // Define how to apply this migration: Create the table.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                sea_query::Index::create()
                    .name(INDEX_NAME)
                    .table(ServiceCheckHistory::Table)
                    .col(ServiceCheckHistory::ServiceCheckId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    // Define how to rollback this migration
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(sea_query::Index::drop().name(INDEX_NAME).to_owned())
            .await
    }
}
