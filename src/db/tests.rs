use crate::db::{get_next_service_check, update_db_from_config};
use crate::prelude::*;

use crate::log::setup_logging;

#[tokio::test]
async fn test_next_service_check() {
    let (db, config) = test_setup().await.expect("Failed to start test harness");

    crate::db::update_db_from_config(db.as_ref(), config.clone())
        .await
        .unwrap();

    let next_check = get_next_service_check(&db).await.unwrap();
    dbg!(&next_check);
    assert!(next_check.is_some());
}

#[cfg(test)]
pub(crate) async fn test_setup() -> Result<(Arc<DatabaseConnection>, SendableConfig), Error> {
    test_setup_harness(true, false).await
}

#[cfg(test)]
pub(crate) async fn test_setup_harness(
    debug: bool,
    db_debug: bool,
) -> Result<(Arc<DatabaseConnection>, SendableConfig), Error> {
    // make sure logging is happening
    let _ = setup_logging(debug, db_debug);
    // enable the rustls crypto provider
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let db = Arc::new(
        crate::db::test_connect()
            .await
            .expect("Failed to connect to database"),
    );

    let config = Configuration::load_test_config().await;

    crate::db::update_db_from_config(&db, config.clone())
        .await
        .expect("Failed to update DB from config");
    Ok((db, config))
}

#[cfg(test)]
pub(crate) async fn test_setup_quieter() -> Result<(Arc<DatabaseConnection>, SendableConfig), Error>
{
    test_setup_harness(false, false).await
}

#[cfg(test)]
pub(crate) async fn test_setup_with_real_db() -> Result<
    (
        tempfile::NamedTempFile,
        Arc<DatabaseConnection>,
        SendableConfig,
    ),
    Error,
> {
    // make sure logging is happening
    let _ = setup_logging(true, true);
    // enable the rustls crypto provider
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let config = Configuration::load_test_config().await;

    let tempfile = tempfile::NamedTempFile::new().expect("Failed to create tempfile");

    // create a temporary filename for this test
    config.write().await.database_file = tempfile
        .path()
        .to_str()
        .expect("Failed to get filepath")
        .to_string();

    let db = Arc::new(
        crate::db::connect(config.clone())
            .await
            .expect("Failed to connect to database"),
    );

    crate::db::update_db_from_config(&db, config.clone())
        .await
        .expect("Failed to update DB from config");
    Ok((tempfile, db, config))
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

#[tokio::test]
async fn test_failing_update_db_from_config() {
    use sea_orm::{DatabaseBackend, MockDatabase};

    let db = MockDatabase::new(DatabaseBackend::Sqlite)
        .append_query_results([[entities::host::Model {
            id: Uuid::new_v4(),
            name: "Apple Pie".to_owned(),
            hostname: "localhost".to_owned(),
            check: crate::host::HostCheck::Ping,
            config: serde_json::json!({}),
        }]])
        .into_connection();

    let res = update_db_from_config(&db, Configuration::load_test_config().await).await;

    dbg!(&res);
    assert!(res.is_err());
}
