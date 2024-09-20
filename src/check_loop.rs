//! Runs the service checks on a loop

use crate::db::get_next_service_check;
use crate::prelude::*;
use entities::service_check::set_check_result;
use opentelemetry::KeyValue;
use sea_orm::prelude::*;
use tokio::sync::Semaphore;

const DEFAULT_BACKOFF: std::time::Duration = tokio::time::Duration::from_millis(50);
const MAX_BACKOFF_TIME: std::time::Duration = tokio::time::Duration::from_secs(1);

#[derive(Clone, Debug)]
/// The end result of a service check
pub struct CheckResult {
    /// When the check finished
    pub timestamp: chrono::DateTime<Utc>,
    /// How long it took
    pub time_elapsed: Duration,
    /// The result
    pub status: ServiceStatus,
    /// Any explanatory/returned text
    pub result_text: String,
}

#[instrument(level = "INFO", skip(db))]
/// Does what it says on the tin
pub(crate) async fn run_service_check(
    db: &DatabaseConnection,
    service_check: &entities::service_check::Model,
    service: entities::service::Model,
) -> Result<(), Error> {
    debug!(
        "service check time! {} - {}",
        service_check.id.hyphenated(),
        service.name
    );

    let check: Service = match Service::try_from_service_model(&service, db).await {
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
        .one(db)
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

    #[cfg(not(tarpaulin_include))]
    let config = check.config().ok_or_else(|| {
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
        service_check.clone(),
        &service,
        chrono::Utc::now(),
        result.status,
        db,
    )
    .await
    .inspect_err(|err| {
        error!(
            "Failed to set status for service check: {:?} - {:?}",
            service_check_id, err
        );
    })?;

    if let Err(err) =
        entities::service_check_history::Model::from_service_check_result(service_check_id, &result)
            .into_active_model()
            .insert(db)
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

#[cfg(not(tarpaulin_include))]
/// Loop around and do the checks, keeping it to a limit based on `max_permits`
pub async fn run_check_loop(
    db: Arc<DatabaseConnection>,
    max_permits: usize,
    metrics_meter: Arc<Meter>,
) -> Result<(), Error> {
    // Create a Counter Instrument.

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
                let db_clone2 = db.clone();
                if let Some((service_check, service)) = get_next_service_check(&db_clone2).await? {
                    let service_check = service_check
                        .set_status(ServiceStatus::Checking, db.as_ref())
                        .await?;
                    let checks_run_since_startup_clone = checks_run_since_startup.clone();
                    let db_clone = db.clone();
                    tokio::spawn(async move {
                        let sc_id = service_check.id.hyphenated().to_string();
                        if let Err(err) =
                            run_service_check(&db_clone, &service_check, service).await
                        {
                            error!("Failed to run service check: {:?}", err);
                            let mut service_check = service_check.into_active_model();
                            service_check.status.set_if_not_equals(ServiceStatus::Error);
                            service_check
                                .last_updated
                                .set_if_not_equals(chrono::Utc::now());

                            if let Err(err) = service_check.update(db_clone.as_ref()).await {
                                error!(
                                    "Failed to update service service_check {} check status to error: {:?}",
                                    sc_id, err
                                );
                            };

                            checks_run_since_startup_clone.add(
                                1,
                                &[KeyValue::new("type", "error"), KeyValue::new("id", sc_id)],
                            );
                        } else {
                            checks_run_since_startup_clone.add(
                                1,
                                &[
                                    KeyValue::new(
                                        "result",
                                        ToString::to_string(&ServiceStatus::Ok),
                                    ),
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
                    trace!("Nothing to do, waiting {}ms", backoff.as_millis());
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
    }
}

#[cfg(test)]
mod tests {
    use entities::service_check;

    use super::*;
    use crate::db::tests::test_setup;

    #[tokio::test]
    async fn test_run_service_check() {
        let (db, _config) = test_setup().await.expect("Failed to setup test");
        let service = entities::service::Entity::find()
            .filter(entities::service::Column::ServiceType.eq(ServiceType::Ping))
            .one(db.as_ref())
            .await
            .expect("Failed to query ping service")
            .expect("Failed to find ping service");

        let service_check = service_check::Entity::find()
            .filter(service_check::Column::ServiceId.eq(service.id))
            .one(db.as_ref())
            .await
            .expect("Failed to query service check")
            .expect("Failed to find service check");

        run_service_check(&db, &service_check, service)
            .await
            .expect("Failed to run service check");
    }

    #[tokio::test]
    async fn test_run_pending_service_check() {
        let (db, _config) = test_setup().await.expect("Failed to setup test");

        service_check::Entity::update_many()
            .col_expr(
                service_check::Column::Status,
                Expr::value(ServiceStatus::Pending),
            )
            .exec(db.as_ref())
            .await
            .expect("Failed to update service checks to pending");

        let service = entities::service::Entity::find()
            .filter(entities::service::Column::ServiceType.eq(ServiceType::Ping))
            .one(db.as_ref())
            .await
            .expect("Failed to query ping service")
            .expect("Failed to find ping service");

        let service_check = service_check::Entity::find()
            .filter(service_check::Column::ServiceId.eq(service.id))
            .one(db.as_ref())
            .await
            .expect("Failed to query service check")
            .expect("Failed to find service check");

        dbg!(&service, &service_check);
        run_service_check(&db, &service_check, service)
            .await
            .expect("Failed to run service check");
    }
}
