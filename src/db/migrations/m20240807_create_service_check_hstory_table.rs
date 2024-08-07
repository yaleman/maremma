use sea_orm::Iterable;
use sea_orm_migration::prelude::*;

use crate::prelude::ServiceStatus;

use super::m20240802_create_service_check_table::ServiceCheck;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20240807_create_service_check_hstory_table" // Make sure this matches with the file name
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // Define how to apply this migration: Create the table.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ServiceCheckHistory::Table)
                    .col(
                        ColumnDef::new(ServiceCheckHistory::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ServiceCheckHistory::Timestamp)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ServiceCheckHistory::ServiceCheckId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ServiceCheckHistory::Status)
                            .enumeration(Alias::new("status"), ServiceStatus::iter())
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ServiceCheckHistory::ResultText)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ServiceCheckHistory::TimeElapsed)
                            .big_unsigned()
                            .not_null(),
                    )
                    // link to the service check
                    .foreign_key(
                        ForeignKey::create()
                            .name("service_check_service_id")
                            .from(
                                ServiceCheckHistory::Table,
                                ServiceCheckHistory::ServiceCheckId,
                            )
                            .to(ServiceCheck::Table, ServiceCheck::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    // Define how to rollback this migration: Drop the table.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ServiceCheckHistory::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
pub enum ServiceCheckHistory {
    Table,
    Id,
    ServiceCheckId,
    Timestamp,
    Status,
    ResultText,
    TimeElapsed,
}
