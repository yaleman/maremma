use crate::db::get_next_service_check;
use crate::prelude::*;
use chrono::Duration;
use sea_orm::prelude::*;

#[derive(Clone, Debug)]
#[allow(dead_code)] // 'cause debug
pub struct CheckResult {
    pub timestamp: chrono::DateTime<Utc>,
    pub time_elapsed: Duration,
    pub status: ServiceStatus,
    pub result_text: String,
}

#[cfg(not(tarpaulin_include))] // TODO: tarpaulin un-ignore for code coverage
pub async fn run_check_loop(db: Arc<DatabaseConnection>) -> Result<(), Error> {
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

            if let Err(err) = service_check
                .set_status(ServiceStatus::Checking, db.as_ref())
                .await
            {
                error!(
                    "Failed to set 'checking' status for service_check_id={} error={:?}",
                    service_check.id.hyphenated(),
                    err
                );
            };

            debug!(
                "service check time! {} - {}",
                service_check.id.hyphenated(),
                service.name
            );

            let check: Service = match (&service).try_into() {
                Ok(check) => check,
                Err(err) => {
                    error!(
                        "Failed to convert service check {} to service: {:?}",
                        service_check.id, err
                    );
                    if let Err(err) = service_check
                        .set_status(ServiceStatus::Error, db.as_ref())
                        .await
                    {
                        error!(
                            "Failed to set 'error' status for service_check_id={} error={:?}",
                            service_check.id.hyphenated(),
                            err
                        );
                    };
                    continue;
                }
            };

            let host: entities::host::Model = match service_check
                .find_related(entities::host::Entity)
                .one(db.as_ref())
                .await?
            {
                Some(host) => {
                    debug!(
                        "Found host: {} for service_check={}",
                        host.name,
                        service_check.id.hyphenated()
                    );
                    host
                }
                None => {
                    error!(
                        "Failed to get host for service check: {:?}",
                        service_check.id
                    );
                    if let Err(err) = service_check
                        .set_status(ServiceStatus::Error, db.as_ref())
                        .await
                    {
                        error!(
                            "Failed to set 'error' status for service_check_id={} error={:?}",
                            service_check.id.hyphenated(),
                            err
                        );
                    };
                    continue;
                }
            };

            let config = check.config.ok_or_else(|| {
                error!(
                    "Failed to get service config for {}",
                    service.id.hyphenated()
                );
                Error::ServiceConfigNotFound(service.id.hyphenated().to_string())
            })?;
            debug!("service={} config: {:?}", service.id.hyphenated(), config);

            let result = match config.run(&host).await {
                Ok(val) => val,
                Err(err) => {
                    error!(
                        "Failed to run service_check_id={} error={:?}",
                        service_check.id.hyphenated(),
                        err
                    );
                    service_check
                        .set_status(ServiceStatus::Error, db.as_ref())
                        .await?;
                    return Err(err);
                }
            };

            // TODO: record result text and status and service_check_id etc
            info!("id={} result={:?}", service_check.id, result);
            service_check
                .set_last_check(chrono::Utc::now(), result.status, db.as_ref())
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
