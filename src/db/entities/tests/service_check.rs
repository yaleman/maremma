use crate::db::entities::{host, service};
use crate::db::get_next_service_check;
use crate::db::tests::test_setup;
use crate::prelude::*;

use core::panic;
use sea_orm::{ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, QueryTrait, TryIntoModel};

#[tokio::test]
async fn test_service_check_entity() {
    let (db, _config) = test_setup().await.expect("Failed to start test harness");

    let service = service::test_service();
    let host = host::test_host();
    info!("saving service...");
    let db_writer = db.write().await;
    let service_am = service.into_active_model();
    let _service = service::Entity::insert(service_am.to_owned())
        .exec(&*db_writer)
        .await
        .expect("Failed to save service");
    let host_am = host.into_active_model();
    let _host = host::Entity::insert(host_am.to_owned())
        .exec(&*db_writer)
        .await
        .expect("Failed to save host");

    let service_check = entities::service_check::Model {
        id: Uuid::new_v4(),
        service_id: service_am.id.clone().unwrap(),
        host_id: host_am.id.clone().unwrap(),
        ..Default::default()
    };

    let service_check_id = service_check.id;

    let am = service_check.into_active_model();

    if let Err(err) = entities::service_check::Entity::insert(am)
        .exec(&*db_writer)
        .await
    {
        panic!("Failed to insert service check: {:?}", err);
    };

    let service_check = entities::service_check::Entity::find()
        .filter(entities::service_check::Column::Id.eq(service_check_id))
        .one(&*db_writer)
        .await
        .expect("Failed to query DB")
        .expect("Failed to find service check");

    info!("found it: {:?}", service_check);

    entities::service_check::Entity::delete_by_id(service_check_id)
        .exec(&*db_writer)
        .await
        .expect("Failed to delete service check");
    // Check we didn't delete the host when deleting the service check
    #[allow(clippy::unwrap_used)]
    let hamid = host_am.id.unwrap();
    assert!(host::Entity::find_by_id(hamid)
        .one(&*db_writer)
        .await
        .expect("Failed to query DB")
        .is_some());
    #[allow(clippy::unwrap_used)]
    let scamid = service_am.id.unwrap();
    assert!(service::Entity::find_by_id(scamid)
        .one(&*db_writer)
        .await
        .expect("Failed to query DB")
        .is_some());
}

#[tokio::test]
/// test creating a service + host + service check, then deleting a host - which should delete the service_check
async fn test_service_check_fk_host() {
    let (db, _config) = test_setup().await.expect("Failed to start test harness");

    let db_writer = db.write().await;

    let service = service::test_service();
    let host = host::test_host();
    info!("saving service...");

    let service_am = service.into_active_model();
    let _service = service::Entity::insert(service_am.to_owned())
        .exec(&*db_writer)
        .await
        .expect("Failed to save service");
    let host_am_id = host.id;
    let host_am = host.into_active_model();
    let _host = host::Entity::insert(host_am.to_owned())
        .exec(&*db_writer)
        .await
        .expect("Failed to save host");

    let service_check = entities::service_check::Model {
        id: Uuid::new_v4(),
        service_id: service_am.id.unwrap(),
        host_id: host_am.id.unwrap(),
        ..Default::default()
    };
    let service_check_am = service_check
        .into_active_model()
        .insert(&*db_writer)
        .await
        .expect("Failed to save service check")
        .try_into_model()
        .expect("Failed to turn activemodel into model");

    assert!(
        entities::service_check::Entity::find_by_id(service_check_am.id)
            .one(&*db_writer)
            .await
            .expect("Failed to query DB")
            .is_some()
    );
    host::Entity::delete_by_id(host_am_id)
        .exec(&*db_writer)
        .await
        .expect("Failed to delete host");
    // Check we delete the service check when deleting the host
    assert!(
        entities::service_check::Entity::find_by_id(service_check_am.id)
            .one(&*db_writer)
            .await
            .expect("Failed to query DB")
            .is_none()
    );
}
#[tokio::test]
/// test creating a service + host + service check, then deleting a host - which should delete the service_check
async fn test_service_check_fk_service() {
    let (db, _config) = test_setup().await.expect("Failed to start test harness");

    let service = service::test_service();
    let host = host::test_host();
    info!("saving service...");
    let db_writer = db.write().await;

    let service_am = service.clone().into_active_model();
    let _service = service::Entity::insert(service_am.to_owned())
        .exec(&*db_writer)
        .await
        .expect("Failed to save service");
    let host_am = host.into_active_model();
    let _host = host::Entity::insert(host_am.clone())
        .exec(&*db_writer)
        .await
        .expect("Failed to save host");

    #[allow(clippy::unwrap_used)]
    let service_check = entities::service_check::Model {
        id: Uuid::new_v4(),
        service_id: service_am.id.unwrap(),
        host_id: host_am.id.unwrap(),
        ..Default::default()
    };
    let service_check_am = service_check.into_active_model();
    dbg!(&service_check_am);
    if let Err(err) = entities::service_check::Entity::insert(service_check_am.to_owned())
        .exec(&*db_writer)
        .await
    {
        panic!("Failed to insert service check: {:?}", err);
    };

    #[allow(clippy::unwrap_used)]
    let scamid = service_check_am.id.clone().unwrap();
    assert!(entities::service_check::Entity::find_by_id(scamid)
        .one(&*db_writer)
        .await
        .expect("Failed to query DB")
        .is_some());
    service::Entity::delete_by_id(service.id)
        .exec(&*db_writer)
        .await
        .expect("Failed to delete service");
    // Check we delete the service check when deleting the service
    assert!(
        entities::service_check::Entity::find_by_id(service_check_am.id.unwrap())
            .one(&*db_writer)
            .await
            .expect("Failed to query DB")
            .is_none()
    );
}

#[tokio::test]
async fn test_full_service_check() {
    let (db, _config) = test_setup().await.expect("Failed to set up test config");

    let known_service_check_service_id = entities::service_check::Entity::find()
        .all(&*db.write().await)
        .await
        .expect("Failed to query DB")
        .into_iter()
        .next()
        .expect("Failed to find service check")
        .service_id;

    info!(
        "We know we have a service check with service_id: {}",
        known_service_check_service_id
    );

    let query = entities::service_check::FullServiceCheck::get_by_service_id_query(
        known_service_check_service_id,
    )
    .build((*db.read().await).get_database_backend());
    info!("Query: {}", query);

    let service_check = entities::service_check::FullServiceCheck::get_by_service_id(
        known_service_check_service_id,
        &*db.write().await,
    )
    .await
    .expect("Failed to get service_check");

    info!("found service check {:?}", service_check);

    assert!(!service_check.is_empty());
}

#[tokio::test]
async fn test_get_urgent_service_check() {
    let (db, _config) = test_setup().await.expect("Failed to setup test db");

    let sc = entities::service_check::Entity::find()
        .one(&*db.write().await)
        .await
        .expect("Failed to query DB")
        .expect("Failed to get service check");

    sc.set_status(ServiceStatus::Urgent, &*db.write().await)
        .await
        .expect("Failed to set status to urgent");

    let urgent = get_next_service_check(&*db.write().await)
        .await
        .expect("Failed to query DB");
    assert!(urgent.is_some());

    let (sc, _) = urgent.expect("Failed to get next service check");
    assert_eq!(sc.status, ServiceStatus::Urgent);
}

#[tokio::test]
async fn test_get_next_pending_service_check() {
    let (db, _config) = test_setup().await.expect("Failed to setup test db");

    let sc = entities::service_check::Entity::find()
        .one(&*db.write().await)
        .await
        .expect("Failed to query DB")
        .expect("Failed to get service check");

    sc.set_status(ServiceStatus::Pending, &*db.write().await)
        .await
        .expect("Failed to set status to pending");

    let urgent = get_next_service_check(&*db.write().await)
        .await
        .expect("Failed to query DB");
    assert!(urgent.is_some());

    let (sc, _) = urgent.expect("Failed to get next service check");
    assert_eq!(sc.status, ServiceStatus::Pending);
}
