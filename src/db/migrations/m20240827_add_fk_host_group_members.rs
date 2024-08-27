//! Adding foreign key constraints between host_group_members and host / host_group tables.

// use sea_orm::sea_query::{Table, TableForeignKey};
use sea_orm::DbErr;
// use sea_orm::{DbErr, ForeignKeyAction};
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20240827_add_fk_host_group_members" // Make sure this matches with the file name
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // Define how to apply this migration: Create the table.
    async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // TODO: work out how to do this - need to rename the table to something else, create the "new" table with the foreign keys, then copy the data over

        // let fk_host_group = TableForeignKey::new()
        //     .name("host_group_members_host_group_fk")
        //     .from_tbl(super::m20240802_create_host_group_members_table::HostGroupMembers::Table)
        //     .from_col(super::m20240802_create_host_group_members_table::HostGroupMembers::GroupId)
        //     .to_tbl(super::m20240802_create_host_group_table::HostGroup::Table)
        //     .to_col(super::m20240802_create_host_group_table::HostGroup::Id)
        //     .on_delete(ForeignKeyAction::Cascade)
        //     .on_update(ForeignKeyAction::Cascade)
        //     .to_owned();
        // let fk_host = TableForeignKey::new()
        //     .name("host_group_members_host_fk")
        //     .from_tbl(super::m20240802_create_host_group_members_table::HostGroupMembers::Table)
        //     .from_col(super::m20240802_create_host_group_members_table::HostGroupMembers::HostId)
        //     .to_tbl(super::m20240802_create_host_table::Host::Table)
        //     .to_col(super::m20240802_create_host_table::Host::Id)
        //     .on_delete(ForeignKeyAction::Cascade)
        //     .on_update(ForeignKeyAction::Cascade)
        //     .to_owned();

        // manager
        //     .alter_table(
        //         Table::alter()
        //             .add_foreign_key(&fk_host_group)
        //             .table(
        //                 super::m20240802_create_host_group_members_table::HostGroupMembers::Table,
        //             )
        //             .to_owned(),
        //     )
        //     .await?;
        // manager
        //     .alter_table(
        //         Table::alter()
        //             .add_foreign_key(&fk_host)
        //             .table(
        //                 super::m20240802_create_host_group_members_table::HostGroupMembers::Table,
        //             )
        //             .to_owned(),
        //     )
        //     .await?;

        Ok(())
    }

    // Define how to rollback this migration
    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // TODO: reverse migration
        Ok(())
    }
}

// #[derive(Iden)]
// pub enum HostGroupMembers {
//     Table,
//     HostGroupId,
// }
