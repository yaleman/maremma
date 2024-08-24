use crate::db::get_next_service_check;
use crate::prelude::*;

use crate::log::setup_logging;

#[tokio::test]
async fn test_next_service_check() {
    let (db, config) = test_setup().await.expect("Failed to start test harness");

    crate::db::update_db_from_config(db.clone(), config.clone())
        .await
        .unwrap();

    let next_check = get_next_service_check(&db).await.unwrap();
    dbg!(&next_check);
    assert!(next_check.is_some());
}

#[cfg(test)]
pub(crate) async fn test_setup() -> Result<(Arc<DatabaseConnection>, Arc<Configuration>), Error> {
    // make sure logging is happening
    let _ = setup_logging(true, true);
    // enable the rustls crypto provider
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let db = Arc::new(
        crate::db::test_connect()
            .await
            .expect("Failed to connect to database"),
    );

    let config = Configuration::load_test_config().await;

    crate::db::update_db_from_config(db.clone(), config.clone())
        .await
        .expect("Failed to update DB from config");
    Ok((db, config))
}

#[tokio::test]
async fn test_get_related() {
    let (db, _config) = test_setup().await.expect("Failed to start test harness");

    for host in entities::host::Entity::find()
        .all(db.as_ref())
        .await
        .unwrap()
        .into_iter()
    {
        info!("Found host: {:?}", host);

        let host_group_members = entities::host_group_members::Entity::find()
            .all(db.as_ref())
            .await
            .unwrap();

        info!("Found host_group_members: {:?}", host_group_members);

        let linked = host
            .find_linked(entities::host_group_members::HostToGroups)
            .all(db.as_ref())
            .await
            .expect("Failed to find linked");
        println!("linked {:?}", linked);
    }
}
