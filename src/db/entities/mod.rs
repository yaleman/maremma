use crate::prelude::*;
use sea_orm::prelude::*;

pub mod host;
pub mod host_group;
pub mod host_group_members;
pub mod service;
pub mod service_check;
pub mod service_check_history;
pub mod service_group_link;
pub mod service_v1;
pub mod session;
#[cfg(test)]
pub mod tests;
pub mod user;

#[async_trait]
pub trait MaremmaEntity {
    async fn update_db_from_config(
        db: &DatabaseConnection,
        config: SendableConfig,
    ) -> Result<(), Error>;

    async fn find_by_name(name: &str, db: &DatabaseConnection) -> Result<Option<Self>, Error>
    where
        Self: Sized;
}
