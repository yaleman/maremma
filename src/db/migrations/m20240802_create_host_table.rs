use sea_orm::Iterable;
use sea_orm_migration::prelude::*;

use crate::host::HostCheck;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20240802_create_host_table" // Make sure this matches with the file name
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // Define how to apply this migration: Create the table.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Host::Table)
                    .col(ColumnDef::new(Host::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Host::Name).string().not_null())
                    .col(ColumnDef::new(Host::Hostname).string())
                    .col(
                        ColumnDef::new(Host::Check)
                            .enumeration(Alias::new("check"), HostCheck::iter())
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    // Define how to rollback this migration: Drop the table.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Host::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
pub enum Host {
    Table,
    Id,
    Name,
    Hostname,
    Check,
}
