use crate::prelude::*;
use sea_orm::prelude::*;

pub mod host;
pub mod host_group;
pub mod host_group_members;
pub mod service;
pub mod service_check;

#[async_trait]
pub trait MaremmaEntity {
    async fn update_db_from_config(
        db: Arc<DatabaseConnection>,
        config: &Configuration,
    ) -> Result<(), Error>;
}
