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

#[cfg(test)]
mod tests {
    use sea_orm_migration::MigratorTrait;

    #[tokio::test]
    async fn test_migrator() {
        let db = crate::db::test_connect()
            .await
            .expect("Failed to connect to test DB");

        super::Migrator::refresh(&db)
            .await
            .expect("Failed to run migrations");
    }
}
