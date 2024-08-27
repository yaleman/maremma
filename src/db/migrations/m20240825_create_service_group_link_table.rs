use std::collections::HashMap;

use sea_orm::{ActiveModelBehavior, ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter};
use sea_orm_migration::prelude::*;

use tracing::debug;
use uuid::Uuid;

use crate::db::entities;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20240822_create_service_group_link_table" // Make sure this matches with the file name
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // Define how to apply this migration: Create the table.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ServiceGroupLink::Table)
                    .col(
                        ColumnDef::new(ServiceGroupLink::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ServiceGroupLink::ServiceId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ServiceGroupLink::GroupId).uuid().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("service_group_link_service_id")
                            .from(ServiceGroupLink::Table, ServiceGroupLink::ServiceId)
                            .to(
                                super::m20240802_create_service_table::Service::Table,
                                super::m20240802_create_service_table::Service::Id,
                            )
                            .on_update(ForeignKeyAction::Cascade)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("service_group_link_group_id")
                            .from(ServiceGroupLink::Table, ServiceGroupLink::GroupId)
                            .to(
                                super::m20240802_create_host_group_table::HostGroup::Table,
                                super::m20240802_create_host_group_table::HostGroup::Id,
                            )
                            .on_update(ForeignKeyAction::Cascade)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        let db = manager.get_connection();
        let services = entities::service_v1::Entity::find().all(db).await?;

        let mut service_group_pairs: Vec<(Uuid, String)> = Vec::new();

        for service in services {
            let service_id = service.id;
            let host_groups: Vec<String> = serde_json::from_value(service.host_groups)
                .map_err(|err| DbErr::Custom(err.to_string()))?;

            for host_group in host_groups {
                service_group_pairs.push((service_id, host_group));
            }
        }

        let mut group_ids = HashMap::<String, Uuid>::new();

        for (service_id, host_group) in service_group_pairs {
            // do we have it cached?
            let mut group_id = group_ids.get(&host_group).cloned();

            if group_id.is_none() {
                // check the db
                if let Some(host_group) = entities::host_group::Entity::find()
                    .filter(entities::host_group::Column::Name.eq(&host_group))
                    .one(db)
                    .await?
                {
                    group_ids.insert(host_group.name.clone(), host_group.id);
                    group_id = Some(host_group.id);

                    // ensure the service_group_link record exists
                    if entities::service_group_link::Entity::find()
                        .filter(
                            entities::service_group_link::Column::ServiceId
                                .eq(service_id)
                                .and(
                                    entities::service_group_link::Column::GroupId.eq(host_group.id),
                                ),
                        )
                        .one(db)
                        .await?
                        .is_none()
                    {
                        let mut sglam = entities::service_group_link::ActiveModel::new();
                        sglam.id.set_if_not_equals(Uuid::new_v4());
                        sglam.service_id.set_if_not_equals(service_id);
                        sglam.group_id.set_if_not_equals(host_group.id);
                        debug!(
                            "adding service group link for service_id: {:?}, group_id: {:?}",
                            service_id, host_group
                        );
                        sglam.insert(db).await?;
                    }
                } else {
                    return Err(DbErr::Custom(format!(
                        "Couldn't find the host group {} in the database?",
                        host_group
                    )));
                }
            }
            eprintln!("Found group_id: {:?}", group_id);
        }
        Ok(())
    }

    // Define how to rollback this migration
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // TODO: reverse the migration, need to ensure the data goes back into the service table
        manager
            .drop_table(Table::drop().table(ServiceGroupLink::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
pub enum ServiceGroupLink {
    Table,
    Id,
    ServiceId,
    GroupId,
}
