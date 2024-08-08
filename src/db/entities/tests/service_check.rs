use crate::db::entities::{host, service};
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

    let service_am = service.into_active_model();
    let _service = service::Entity::insert(service_am.to_owned())
        .exec(db.as_ref())
        .await
        .unwrap();
    let host_am = host.into_active_model();
    let _host = host::Entity::insert(host_am.to_owned())
        .exec(db.as_ref())
        .await
        .unwrap();

    let service_check = entities::service_check::Model {
        id: Uuid::new_v4(),
        service_id: service_am.id.clone().unwrap(),
        host_id: host_am.id.clone().unwrap(),
        ..Default::default()
    };

    let service_check_id = service_check.id;

    let am = service_check.into_active_model();

    if let Err(err) = entities::service_check::Entity::insert(am)
        .exec(db.as_ref())
        .await
    {
        panic!("Failed to insert service check: {:?}", err);
    };

    let service_check = entities::service_check::Entity::find()
        .filter(entities::service_check::Column::Id.eq(service_check_id))
        .one(db.as_ref())
        .await
        .unwrap()
        .unwrap();

    info!("found it: {:?}", service_check);

    entities::service_check::Entity::delete_by_id(service_check_id)
        .exec(db.as_ref())
        .await
        .unwrap();
    // Check we didn't delete the host when deleting the service check
    assert!(host::Entity::find_by_id(host_am.id.unwrap())
        .one(db.as_ref())
        .await
        .unwrap()
        .is_some());
    assert!(service::Entity::find_by_id(service_am.id.unwrap())
        .one(db.as_ref())
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
/// test creating a service + host + service check, then deleting a host - which should delete the service_check
async fn test_service_check_fk_host() {
    let (db, _config) = test_setup().await.expect("Failed to start test harness");

    let service = service::test_service();
    let host = host::test_host();
    info!("saving service...");

    let service_am = service.into_active_model();
    let _service = service::Entity::insert(service_am.to_owned())
        .exec(db.as_ref())
        .await
        .unwrap();
    let host_am_id = host.id;
    let host_am = host.into_active_model();
    let _host = host::Entity::insert(host_am.to_owned())
        .exec(db.as_ref())
        .await
        .unwrap();

    let service_check = entities::service_check::Model {
        id: Uuid::new_v4(),
        service_id: service_am.id.unwrap(),
        host_id: host_am.id.unwrap(),
        ..Default::default()
    };
    let service_check_am = service_check
        .into_active_model()
        .insert(db.as_ref())
        .await
        .expect("Failed to save service check")
        .try_into_model()
        .expect("Failed to turn activemodel into model");

    assert!(
        entities::service_check::Entity::find_by_id(service_check_am.id)
            .one(db.as_ref())
            .await
            .unwrap()
            .is_some()
    );
    host::Entity::delete_by_id(host_am_id)
        .exec(db.as_ref())
        .await
        .unwrap();
    // Check we delete the service check when deleting the host
    assert!(
        entities::service_check::Entity::find_by_id(service_check_am.id)
            .one(db.as_ref())
            .await
            .unwrap()
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

    let service_am = service.clone().into_active_model();
    let _service = service::Entity::insert(service_am.to_owned())
        .exec(db.as_ref())
        .await
        .unwrap();
    let host_am = host.into_active_model();
    let _host = host::Entity::insert(host_am.clone())
        .exec(db.as_ref())
        .await
        .unwrap();

    let service_check = entities::service_check::Model {
        id: Uuid::new_v4(),
        service_id: service_am.id.unwrap(),
        host_id: host_am.id.unwrap(),
        ..Default::default()
    };
    let service_check_am = service_check.into_active_model();
    dbg!(&service_check_am);
    if let Err(err) = entities::service_check::Entity::insert(service_check_am.to_owned())
        .exec(db.as_ref())
        .await
    {
        panic!("Failed to insert service check: {:?}", err);
    };

    assert!(
        entities::service_check::Entity::find_by_id(service_check_am.id.clone().unwrap())
            .one(db.as_ref())
            .await
            .unwrap()
            .is_some()
    );
    service::Entity::delete_by_id(service.id)
        .exec(db.as_ref())
        .await
        .unwrap();
    // Check we delete the service check when deleting the service
    assert!(
        entities::service_check::Entity::find_by_id(service_check_am.id.unwrap())
            .one(db.as_ref())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn test_full_service_check() {
    let (db, config) = test_setup().await.expect("Failed to set up test config");

    crate::db::update_db_from_config(db.clone(), config.clone())
        .await
        .unwrap();

    let known_service_check_service_id = entities::service_check::Entity::find()
        .all(db.as_ref())
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap()
        .service_id;

    info!(
        "We know we have a service check with service_id: {}",
        known_service_check_service_id
    );

    let query = entities::service_check::FullServiceCheck::get_by_service_id_query(
        known_service_check_service_id,
    )
    .build((*db).get_database_backend());
    info!("Query: {}", query);

    let service_check = entities::service_check::FullServiceCheck::get_by_service_id(
        known_service_check_service_id,
        db.as_ref(),
    )
    .await
    .expect("Failed to get service_check");

    info!("found service check {:?}", service_check);

    assert!(service_check.len() > 0);
}
