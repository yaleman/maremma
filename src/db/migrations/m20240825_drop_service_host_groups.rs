//! Dropping the host_groups column from the Service table

use sea_orm::sea_query::{self, Alias, ColumnDef, Table};
use sea_orm::{DbErr, Iden};
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20240825_drop_service_host_groups" // Make sure this matches with the file name
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // Define how to apply this migration: Create the table.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .drop_column(Alias::new("host_groups"))
                    .table(Service::Table)
                    .to_owned(),
            )
            .await
    }

    // Define how to rollback this migration
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .add_column(ColumnDef::new(Service::HostGroups).string().not_null())
                    .table(Service::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
pub enum Service {
    Table,
    HostGroups,
}
