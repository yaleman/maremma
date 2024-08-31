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
            Box::new(super::migrations::m20240807_create_service_check_hstory_table::Migration),
            Box::new(super::migrations::m20240810_create_user_table::Migration),
            Box::new(super::migrations::m20240822_create_session_table::Migration),
            Box::new(super::migrations::m20240825_create_service_group_link_table::Migration),
            Box::new(super::migrations::m20240825_drop_service_host_groups::Migration),
            Box::new(super::migrations::m20240827_add_host_config_column::Migration),
            Box::new(super::migrations::m20240827_add_fk_host_group_members::Migration),
        ]
    }
}

#[cfg(test)]
mod tests {
    use sea_orm_migration::MigratorTrait;

    #[tokio::test]
    async fn test_migrator() {
        let db = crate::db::test_connect()
            .await
            .expect("Failed to connect to test DB");

        super::Migrator::up(&db, None)
            .await
            .expect("Failed to run migrations");
    }
}
