use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20240802_create_host_group_members_table" // Make sure this matches with the file name
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // Define how to apply this migration: Create the Host table.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(HostGroupMembers::Table)
                    .col(
                        ColumnDef::new(HostGroupMembers::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(HostGroupMembers::HostId).uuid().not_null())
                    .col(ColumnDef::new(HostGroupMembers::GroupId).uuid().not_null())
                    .to_owned(),
            )
            .await
    }

    // Define how to rollback this migration: Drop the table.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(HostGroupMembers::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
pub enum HostGroupMembers {
    Table,
    Id,
    HostId,
    GroupId,
}
