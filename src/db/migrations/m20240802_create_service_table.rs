use sea_orm::Iterable;
use sea_orm_migration::prelude::*;

use crate::prelude::ServiceType;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20240802_create_service_table" // Make sure this matches with the file name
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // Define how to apply this migration: Create the table.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Service::Table)
                    .col(ColumnDef::new(Service::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Service::Name).string().not_null())
                    .col(ColumnDef::new(Service::Description).string())
                    .col(ColumnDef::new(Service::HostGroups).string().not_null())
                    .col(ColumnDef::new(Service::CronSchedule).string().not_null())
                    .col(
                        ColumnDef::new(Service::ServiceType)
                            .enumeration(Alias::new("service_type"), ServiceType::iter())
                            .string(),
                    )
                    .col(ColumnDef::new(Service::ExtraConfig).json())
                    .to_owned(),
            )
            .await
    }

    // Define how to rollback this migration: Drop the table.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Service::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
pub enum Service {
    Table,
    Id,
    Name,
    Description,
    HostGroups,
    #[allow(clippy::enum_variant_names)]
    ServiceType,
    CronSchedule,
    ExtraConfig,
}
