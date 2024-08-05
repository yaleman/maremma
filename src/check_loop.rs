use crate::db::get_next_service_check;
use crate::prelude::*;
use sea_orm::prelude::*;

#[cfg(not(tarpaulin_include))] // TODO: un-ignore for code coverage
pub async fn run_check_loop(
    _config: Arc<Configuration>,
    db: Arc<DatabaseConnection>,
) -> Result<(), Error> {
    let mut backoff = tokio::time::Duration::from_millis(50);

    loop {
        if let Some((service_check, service)) = get_next_service_check(db.as_ref()).await? {
            let service = match service {
                Some(service) => service,
                None => {
                    error!(
                        "Failed to get service for service check: {:?}",
                        service_check.id
                    );
                    continue;
                }
            };
            service_check
                .set_status(ServiceStatus::Checking, db.as_ref())
                .await?;

            info!(
                "service check time! {} - {}",
                service_check.id, service.name
            );

            let check: Service = match (&service).try_into() {
                Ok(check) => check,
                Err(err) => {
                    error!(
                        "Failed to convert service check {} to service: {:?}",
                        service_check.id, err
                    );
                    // TODO: if this fails it will leave the service in "checking" status
                    service_check
                        .set_status(ServiceStatus::Error, db.as_ref())
                        .await?;
                    continue;
                }
            };

            let host: entities::host::Model = match service_check
                .find_related(entities::host::Entity)
                .one(db.as_ref())
                .await?
            {
                Some(host) => host,
                None => {
                    error!(
                        "Failed to get host for service check: {:?}",
                        service_check.id
                    );
                    service_check
                        .set_status(ServiceStatus::Error, db.as_ref())
                        .await?;
                    continue;
                }
            };

            let (result_text, status) =
                run_check(db.as_ref(), &service_check, &check, &host).await?;

            // TODO: record result text and status and service_check_id etc
            info!(
                "id={} result_text=\"{}\", status={}",
                service_check.id, result_text, status
            );
            service_check
                .set_last_check(chrono::Utc::now(), status, db.as_ref())
                .await
                .map_err(|err| {
                    error!(
                        "Failed to set status for service check: {:?}",
                        service_check.id
                    );
                    err
                })?;
            service_check.set_next_check(&service, db.as_ref()).await?;
            // reset our backoff time
            backoff = tokio::time::Duration::from_millis(50);
        } else {
            backoff += tokio::time::Duration::from_millis(50);
            if backoff > tokio::time::Duration::from_secs(1) {
                backoff = tokio::time::Duration::from_secs(1);
            }
            debug!("Nothing to do, waiting {}ms", backoff.as_millis());
            tokio::time::sleep(backoff).await;
        }
    }
}

#[instrument(skip_all, fields(service_check_id=service_check.id.hyphenated().to_string(), service_id=service.id.hyphenated().to_string(), hostname=host.hostname))]
pub async fn run_check(
    db: &DatabaseConnection,
    service_check: &entities::service_check::Model,
    service: &Service,
    host: &entities::host::Model,
) -> Result<(String, ServiceStatus), Error> {
    info!("Starting Check: {:?} -> ", service_check.id);

    let _ = db.clone();

    Ok(("woo".to_string(), ServiceStatus::Unknown))
}
