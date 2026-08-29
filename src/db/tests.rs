use crate::db::{get_next_service_check, update_db_from_config};
use crate::prelude::*;

use crate::log::setup_logging;

#[tokio::test]
async fn test_next_service_check() {
    let (db, _config) = test_setup().await.expect("Failed to start test harness");

    let next_check = get_next_service_check(db.as_ref())
        .await
        .expect("Failed to get next check");
    dbg!(&next_check);
    assert!(next_check.is_some());
}

pub(crate) async fn test_setup() -> Result<(Arc<DatabaseConnection>, SendableConfig), MaremmaError>
{
    test_setup_harness(true, false).await
}

pub(crate) async fn test_setup_harness(
    debug: bool,
    db_debug: bool,
) -> Result<(Arc<DatabaseConnection>, SendableConfig), MaremmaError> {
    // make sure logging is happening

    let _ = setup_logging(debug, db_debug, false);
    // enable the rustls crypto provider
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let db = Arc::new(
        crate::db::test_connect()
            .await
            .expect("Failed to connect to database"),
    );

    let config = Configuration::load_test_config().await;

    crate::db::update_db_from_config(db.as_ref(), config.clone())
        .await
        .expect("Failed to update DB from config");
    Ok((db, config))
}

pub(crate) async fn test_setup_quieter(
) -> Result<(Arc<DatabaseConnection>, SendableConfig), MaremmaError> {
    test_setup_harness(false, false).await
}

pub(crate) async fn test_setup_with_real_db() -> Result<
    (
        tempfile::NamedTempFile,
        Arc<DatabaseConnection>,
        SendableConfig,
    ),
    MaremmaError,
> {
    // make sure logging is happening
    let _ = setup_logging(true, true, false);
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

    crate::db::update_db_from_config(db.as_ref(), config.clone())
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
        .expect("Failed to query hosts")
        .into_iter()
    {
        info!("Found host: {:?}", host);

        let host_group_members = entities::host_group_members::Entity::find()
            .all(db.as_ref())
            .await
            .expect("Failed to query host_group_members");

        info!("Found host_group_members: {:?}", host_group_members);

        let linked = host
            .find_linked(entities::host_group_members::HostToGroups)
            .all(db.as_ref())
            .await
            .expect("Failed to find linked");
        println!("linked {linked:?}");
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

#[tokio::test]
async fn update_db_from_config_prunes_removed_configuration() {
    let (db, config) = test_setup().await.expect("Failed to start test harness");

    let example_host = entities::host::Entity::find()
        .filter(entities::host::Column::Name.eq("example.com"))
        .one(db.as_ref())
        .await
        .expect("Failed to query example host")
        .expect("Failed to find example host");
    let tls_service = entities::service::Entity::find()
        .filter(entities::service::Column::Name.eq("check_tls"))
        .one(db.as_ref())
        .await
        .expect("Failed to query TLS service")
        .expect("Failed to find TLS service");
    let ping_service = entities::service::Entity::find()
        .filter(entities::service::Column::Name.eq("ping_check"))
        .one(db.as_ref())
        .await
        .expect("Failed to query ping service")
        .expect("Failed to find ping service");
    let tls_group = entities::host_group::Entity::find()
        .filter(entities::host_group::Column::Name.eq("check_tls"))
        .one(db.as_ref())
        .await
        .expect("Failed to query TLS group")
        .expect("Failed to find TLS group");
    let ntp_group = entities::host_group::Entity::find()
        .filter(entities::host_group::Column::Name.eq("check_ntp_time"))
        .one(db.as_ref())
        .await
        .expect("Failed to query NTP group")
        .expect("Failed to find NTP group");
    let local_service = entities::service::Entity::find()
        .filter(entities::service::Column::Name.eq("local_lslah"))
        .one(db.as_ref())
        .await
        .expect("Failed to query local service")
        .expect("Failed to find local service");
    let local_check_id = entities::service_check::Entity::find()
        .filter(entities::service_check::Column::ServiceId.eq(local_service.id))
        .one(db.as_ref())
        .await
        .expect("Failed to query local service check")
        .expect("Failed to find local service check")
        .id;

    {
        let mut config = config.write().await;
        config
            .services
            .get_mut("ping_check")
            .expect("Failed to find ping service configuration")
            .host_groups = vec!["check_tls".to_string()];
        config
            .hosts
            .get_mut("example.com")
            .expect("Failed to find example host configuration")
            .host_groups
            .retain(|group| group != "check_tls");
    }
    update_db_from_config(db.as_ref(), config.clone())
        .await
        .expect("Failed to reconcile changed configuration");

    assert!(entities::host_group_members::Entity::find()
        .filter(entities::host_group_members::Column::HostId.eq(example_host.id))
        .filter(entities::host_group_members::Column::GroupId.eq(tls_group.id))
        .one(db.as_ref())
        .await
        .expect("Failed to query removed host-group membership")
        .is_none());
    assert!(entities::service_group_link::Entity::find()
        .filter(entities::service_group_link::Column::ServiceId.eq(ping_service.id))
        .filter(entities::service_group_link::Column::GroupId.eq(ntp_group.id))
        .one(db.as_ref())
        .await
        .expect("Failed to query removed service-group link")
        .is_none());
    assert!(entities::service_group_link::Entity::find()
        .filter(entities::service_group_link::Column::ServiceId.eq(ping_service.id))
        .filter(entities::service_group_link::Column::GroupId.eq(tls_group.id))
        .one(db.as_ref())
        .await
        .expect("Failed to query replacement service-group link")
        .is_some());
    assert!(entities::service_check::Entity::find()
        .filter(entities::service_check::Column::HostId.eq(example_host.id))
        .filter(entities::service_check::Column::ServiceId.eq(tls_service.id))
        .one(db.as_ref())
        .await
        .expect("Failed to query removed TLS check")
        .is_none());
    assert_eq!(
        entities::service_check::Entity::find()
            .filter(entities::service_check::Column::ServiceId.eq(local_service.id))
            .one(db.as_ref())
            .await
            .expect("Failed to query preserved local check")
            .expect("Failed to find preserved local check")
            .id,
        local_check_id
    );

    {
        let mut config = config.write().await;
        config.services.remove("ping_check");
        config.hosts.remove("example.com");
    }
    update_db_from_config(db.as_ref(), config.clone())
        .await
        .expect("Failed to reconcile removed host");

    assert!(entities::host::Entity::find_by_id(example_host.id)
        .one(db.as_ref())
        .await
        .expect("Failed to query removed host")
        .is_none());
    assert!(entities::service::Entity::find_by_id(ping_service.id)
        .one(db.as_ref())
        .await
        .expect("Failed to query removed service")
        .is_none());
    assert!(entities::service_check::Entity::find()
        .filter(entities::service_check::Column::HostId.eq(example_host.id))
        .one(db.as_ref())
        .await
        .expect("Failed to query checks for removed host")
        .is_none());
}
