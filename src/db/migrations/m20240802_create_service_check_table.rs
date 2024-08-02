use sea_orm::Iterable;
// use sea_orm::Iterable;
use sea_orm_migration::prelude::*;

use crate::prelude::ServiceStatus;

use super::m20240802_create_host_table::Host;
use super::m20240802_create_service_table::Service;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20240802_create_service_check_table" // Make sure this matches with the file name
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // Define how to apply this migration: Create the Host table.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ServiceCheck::Table)
                    .col(
                        ColumnDef::new(ServiceCheck::Id)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ServiceCheck::Status)
                            .enumeration(Alias::new("status"), ServiceStatus::iter())
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ServiceCheck::LastUpdated)
                            .timestamp()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ServiceCheck::LastCheck)
                            .timestamp()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ServiceCheck::HostId).string().not_null())
                    .col(ColumnDef::new(ServiceCheck::ServiceId).string().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("service_check_service_id")
                            .from(ServiceCheck::Table, ServiceCheck::ServiceId)
                            .to(Service::Table, Service::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("service_check_host_id")
                            .from(ServiceCheck::Table, ServiceCheck::HostId)
                            .to(Host::Table, Host::Id)
                            .on_update(ForeignKeyAction::Cascade)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    // Define how to rollback this migration: Drop the table.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ServiceCheck::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
pub enum ServiceCheck {
    Table,
    Id,
    HostId,
    ServiceId,
    Status,
    LastUpdated,
    LastCheck,
}
