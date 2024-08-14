use crate::db::get_next_service_check;
use crate::prelude::*;
use entities::service_check::set_check_result;
use sea_orm::prelude::*;
use tokio::sync::Semaphore;

const DEFAULT_BACKOFF: std::time::Duration = tokio::time::Duration::from_millis(50);
const MAX_BACKOFF_TIME: std::time::Duration = tokio::time::Duration::from_secs(1);

#[derive(Clone, Debug)]
pub struct CheckResult {
    pub timestamp: chrono::DateTime<Utc>,
    pub time_elapsed: Duration,
    pub status: ServiceStatus,
    pub result_text: String,
}

#[instrument(level = "INFO", skip(db))]
pub(crate) async fn run_service_check(
    db: Arc<DatabaseConnection>,
    service_check: entities::service_check::Model,
    service: Option<entities::service::Model>,
) -> Result<(), Error> {
    let service = match service {
        Some(service) => service,
        None => {
            error!(
                "Failed to get service for service check: {:?}",
                service_check.id
            );
            return Err(Error::ServiceNotFound(service_check.service_id));
        }
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
            return Err(Error::Generic(format!(
                "Failed to convert service check {} to service: {:?}",
                service_check.id, err
            )));
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
            return Err(Error::HostNotFound(service_check.host_id));
        }
    };

    let config = check.config.ok_or_else(|| {
        error!(
            "Failed to get service config for {}",
            service.id.hyphenated()
        );
        Error::ServiceConfigNotFound(service.id.hyphenated().to_string())
    })?;

    debug!("starting service_check={:?}", service_check);
    let result = match config.run(&host).await {
        Ok(val) => val,
        Err(err) => CheckResult {
            timestamp: chrono::Utc::now(),
            time_elapsed: Duration::zero(),
            status: ServiceStatus::Error,
            result_text: format!("Error: {:?}", err),
        },
    };
    debug!(
        "done service_check={:?} result={:?}",
        service_check, result.status
    );
    let service_check_id = service_check.id;

    set_check_result(
        service_check,
        &service,
        chrono::Utc::now(),
        result.status,
        db.as_ref(),
    )
    .await
    .map_err(|err| {
        error!(
            "Failed to set status for service check: {:?}",
            service_check_id
        );
        err
    })?;

    if let Err(err) =
        entities::service_check_history::Model::from_service_check_result(service_check_id, &result)
            .into_active_model()
            .insert(db.as_ref())
            .await
    {
        error!(
            "Failed to store service_check_history for service_check_id={} error={}",
            service_check_id,
            err.to_string()
        )
    }
    Ok(())
}
#[cfg(not(tarpaulin_include))] // TODO: tarpaulin un-ignore for code coverage
pub async fn run_check_loop(
    db: Arc<DatabaseConnection>,
    max_permits: usize,
    metrics_meter: Arc<Meter>,
) -> Result<(), Error> {
    // Create a Counter Instrument.

    use opentelemetry::KeyValue;
    let checks_run_since_startup =
        Arc::new(metrics_meter.u64_counter("checks_run_since_startup").init());

    let mut backoff = DEFAULT_BACKOFF;
    // Limit to n concurrent tasks
    let semaphore = Arc::new(Semaphore::new(max_permits));
    info!("Max concurrent tasks set to {}", max_permits);
    loop {
        while semaphore.available_permits() == 0 {
            warn!("No spare task slots, something might be running slow!");
            tokio::time::sleep(backoff).await;
        }
        match semaphore.clone().acquire_owned().await {
            Ok(permit) => {
                if let Some((service_check, service)) = get_next_service_check(db.as_ref()).await? {
                    let service_check = service_check
                        .set_status(ServiceStatus::Checking, db.as_ref())
                        .await?;
                    let checks_run_since_startup_clone = checks_run_since_startup.clone();
                    let db_clone = db.clone();
                    tokio::spawn(async move {
                        let sc_id = service_check.id.hyphenated().to_string();
                        if let Err(err) = run_service_check(db_clone, service_check, service).await
                        {
                            error!("Failed to run service check: {:?}", err);
                            checks_run_since_startup_clone.add(
                                1,
                                &[KeyValue::new("type", "error"), KeyValue::new("id", sc_id)],
                            );
                        } else {
                            checks_run_since_startup_clone.add(
                                1,
                                &[
                                    KeyValue::new("result", "success"),
                                    KeyValue::new("id", sc_id),
                                ],
                            );
                        }
                    });
                } else {
                    backoff += DEFAULT_BACKOFF;
                    if backoff > MAX_BACKOFF_TIME {
                        backoff = MAX_BACKOFF_TIME;
                    }
                    debug!("Nothing to do, waiting {}ms", backoff.as_millis());
                    tokio::time::sleep(backoff).await;
                }
                drop(permit); // Release the permit when the task is done
            }
            Err(err) => {
                error!("Failed to acquire semaphore permit: {:?}", err);
            }
        };
        // we did a thing, so we can reset the back-off time
        backoff = DEFAULT_BACKOFF;

        // TODO: auto-cleanup service checks stuck in "checking state"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::test_setup;

    #[tokio::test]
    async fn test_run_service_check() {
        let (db, _config) = test_setup().await.expect("Failed to setup test");

        let (service_check, service) = get_next_service_check(db.as_ref())
            .await
            .expect("Failed to run next service check")
            .expect("Failed to find next service check");

        run_service_check(db, service_check, service)
            .await
            .expect("Failed to run service check");
    }
}
