use crate::prelude::*;
use sea_orm::prelude::*;

#[cfg(not(tarpaulin_include))] // ignore for code coverage
pub async fn run_check_loop(
    _config: Arc<Configuration>,
    db: Arc<DatabaseConnection>,
) -> Result<(), Error> {
    use crate::db::get_next_service_check;

    loop {
        if let Some(service_check) = get_next_service_check(db.as_ref()).await? {
            info!("service check time! {}", service_check.id);
        }

        //     let next_service_check = match &query_result {
        //         Ok(res) => match res {
        //             Some(val) => val,
        //             None => {
        //                 debug!("No pending service checks found in DB");
        //                 tokio::time::sleep(core::time::Duration::from_secs(1)).await;
        //                 continue;
        //             }
        //         },
        //         Err(err) => {
        //             error!("Failed to query DB for next service check: {:?}", err);
        //             // tokio::time::sleep(core::time::Duration::from_secs(1)).await;
        //             continue;
        //         }
        //     };

        //     info!("Found next service check in DB: {:?}", next_service_check);

        // match config.get_next_service_check().await {
        //     Some(next_check_id) => {
        //         match config.run_check(&next_check_id).await {
        //             Ok((hostname, status)) => {
        //                 let service_id_reader = config.service_checks.read().await;
        //                 let service_id: String = match service_id_reader
        //                     .get(&next_check_id)
        //                     .map(|s| s.service_id.clone())
        //                 {
        //                     Some(val) => val,
        //                     None => {
        //                         error!(
        //                             "Failed to get service_id from next_check_id: {:?}",
        //                             next_check_id
        //                         );
        //                         drop(service_id_reader);
        //                         continue;
        //                     }
        //                 };

        //                 drop(service_id_reader);

        //                 let service = match config.get_service(&service_id) {
        //                     Some(service) => service,
        //                     None => {
        //                         error!("Failed to get service ID: {:?}", service_id);
        //                         continue;
        //                     }
        //                 };

        //                 status.log(&format!(
        //                     "{next_check_id} {hostname} {} {:?}",
        //                     service.name, &status
        //                 ));

        //                 debug!("Checking in service check... {}", &next_check_id);
        //                 if let Some(service_check) =
        //                     config.service_checks.write().await.get_mut(&next_check_id)
        //                 {
        //                     service_check.checkin(status);
        //                 } else {
        //                     error!("Failed to check in service check: {}", next_check_id);
        //                 }
        //             }
        //             Err(err) => {
        //                 error!("{} Failed to run check: {:?}", next_check_id, err);
        //             }
        //         };
        //     }
        //     None => {
        //         let next_wakeup = config.find_next_wakeup().await;

        //         let delta = next_wakeup - chrono::Utc::now();
        //         if delta.num_microseconds().unwrap_or(0) > 0 {
        //             debug!(
        //                 "No checks to run, sleeping for {} seconds",
        //                 delta.num_seconds()
        //             );
        //             tokio::time::sleep(core::time::Duration::from_millis(
        //                 delta.num_milliseconds() as u64
        //             ))
        //             .await;
        //         }
        //     }
        // }
    }
}

pub async fn run_check(
    db: &DatabaseConnection,
    check: &entities::service_check::Model,
) -> Result<(String, ServiceStatus), Error> {
    let _host = match entities::host::Entity::find()
        .filter(entities::host::Column::Id.eq(check.host_id))
        .one(db)
        .await?
    {
        Some(host) => host,
        None => return Err(Error::HostNotFound(check.host_id)),
    };

    let _service = match entities::service::Entity::find()
        .filter(entities::service::Column::Id.eq(check.service_id))
        .one(db)
        .await?
    {
        Some(service) => service,
        None => return Err(Error::ServiceNotFound(check.service_id)),
    };

    todo!()
}
