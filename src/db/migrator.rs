use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(super::migrations::m20240802_create_host_table::Migration),
            Box::new(super::migrations::m20240802_create_host_group_table::Migration),
            Box::new(super::migrations::m20240802_create_host_group_members_table::Migration),
            Box::new(super::migrations::m20240802_create_service_table::Migration),
            Box::new(super::migrations::m20240802_create_service_check_table::Migration),
        ]
    }
}
