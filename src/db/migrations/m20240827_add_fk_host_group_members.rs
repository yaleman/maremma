//! Adding foreign key constraints between host_group_members and host / host_group tables.

use sea_orm::DbErr;
use sea_orm_migration::prelude::*;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use tracing::debug;

use super::m20240802_create_host_group_table::HostGroup;
use super::m20240802_create_host_table::Host;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20240827_add_fk_host_group_members" // Make sure this matches with the file name
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // Define how to apply this migration: Create the table.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // rename the host_group table to something else, create the "new" table with the foreign keys, then copy the data over

        debug!(
            "Renaming table {} to {}",
            HostGroupMembers::Table.to_string(),
            HostGroupMembersBackup::Table.to_string()
        );
        manager
            .rename_table(
                TableRenameStatement::new()
                    .table(HostGroupMembers::Table, HostGroupMembersBackup::Table)
                    .to_owned(),
            )
            .await?;
        debug!(
            "Creating table {} with foreign keys",
            HostGroupMembers::Table.to_string(),
        );
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
                    .foreign_key(
                        ForeignKey::create()
                            .name("hgm_host_fk")
                            .from(HostGroupMembers::Table, HostGroupMembers::HostId)
                            .to(Host::Table, Host::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("hgm_group_fk")
                            .from(HostGroupMembers::Table, HostGroupMembers::GroupId)
                            .to(HostGroup::Table, HostGroup::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // cleaning up the host_group_members table
        debug!("Dropping all rows with mismatching foreign keys in host_group_members");
        let query = format!(
            "
            SELECT id FROM {}
            WHERE id IN (
                SELECT hgm.id
                FROM {} hgm
                LEFT JOIN {} h ON hgm.host_id = h.id
                WHERE h.id IS NULL
        ) OR id IN (
            SELECT hgm.id
            FROM {} hgm
            LEFT JOIN {} hg ON hgm.group_id = hg.id
            WHERE hg.id IS NULL
        );",
            HostGroupMembersBackup::Table.to_string(),
            HostGroupMembersBackup::Table.to_string(),
            Host::Table.to_string(),
            HostGroupMembersBackup::Table.to_string(),
            HostGroup::Table.to_string(),
        );
        let db = manager.get_connection();
        let res = db.execute_unprepared(&query).await?;
        debug!(
            "Found {} rows in {} that'll be ignored",
            res.rows_affected(),
            HostGroupMembersBackup::Table.to_string()
        );

        // pull the contents of the host_group_members_backup table into the new host_group_members table
        // but only if they've got matching foreign keys
        debug!(
            "Copying data from {} to {}",
            HostGroupMembersBackup::Table.to_string(),
            HostGroupMembers::Table.to_string()
        );
        let query = format!(
            "INSERT INTO {} (id, host_id, group_id)
                SELECT hgmb.id, hgmb.host_id, hgmb.group_id
            FROM {} hgmb
            JOIN {} h ON hgmb.host_id = h.id
            JOIN {} g ON hgmb.group_id = g.id;",
            HostGroupMembers::Table.to_string(),
            HostGroupMembersBackup::Table.to_string(),
            Host::Table.to_string(),
            HostGroup::Table.to_string()
        );

        debug!("Query: {}", query);
        let db = manager.get_connection();
        db.execute_unprepared(&query).await?;

        debug!(
            "Dropping backup table {}",
            HostGroupMembersBackup::Table.to_string()
        );

        // drop the host_group_members_backup table
        manager
            .drop_table(
                TableDropStatement::new()
                    .table(HostGroupMembersBackup::Table)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Migration(
            "This migration cannot be rolled back".to_string(),
        ))
    }
}
#[derive(Iden)]
pub enum HostGroupMembersBackup {
    Table,
}

#[derive(Iden)]
pub enum HostGroupMembers {
    Table,
    Id,
    HostId,
    GroupId,
}
