//! Runs the service checks on a loop

use crate::prelude::*;
use opentelemetry::metrics::Counter;
use opentelemetry::KeyValue;
use rand::seq::IteratorRandom;
use tokio::sync::Semaphore;

const DEFAULT_BACKOFF: std::time::Duration = tokio::time::Duration::from_millis(50);
const MAX_BACKOFF: std::time::Duration = tokio::time::Duration::from_secs(1);

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

#[instrument(level = "INFO", skip_all, fields(service_check_id=%service_check.id, service_id=%service.id))]
/// Does what it says on the tin
pub(crate) async fn run_service_check(
    db: Arc<RwLock<DatabaseConnection>>,
    service_check: &entities::service_check::Model,
    service: entities::service::Model,
) -> Result<(), Error> {
    let db_writer = db.write().await;
    let check = match Service::try_from_service_model(&service, &db_writer).await {
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
        .one(&*db_writer)
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
    let service_to_run = check.config().ok_or_else(|| {
        error!(
            "Failed to get service config for {}",
            service.id.hyphenated()
        );
        Error::ServiceConfigNotFound(service.id.hyphenated().to_string())
    })?;
    drop(db_writer);
    debug!("Starting service_check={:?}", service_check);
    let result = match service_to_run.run(&host).await {
        Ok(val) => val,
        Err(err) => CheckResult {
            timestamp: chrono::Utc::now(),
            time_elapsed: Duration::zero(),
            status: ServiceStatus::Error,
            result_text: format!("Error: {:?}", err),
        },
    };
    let jitter = service_to_run.jitter_value();
    debug!(
        "Completed service_check={:?} result={:?}",
        service_check, result.status
    );

    let db_writer = db.write().await;

    entities::service_check_history::Model::from_service_check_result(service_check.id, &result)
        .into_active_model()
        .insert(&*db_writer)
        .await?;

    let mut model = service_check.clone().into_active_model();
    model.last_check.set_if_not_equals(chrono::Utc::now());
    model.status.set_if_not_equals(result.status);

    // get a number between 0 and jitter
    let jitter: i64 = (0..jitter).choose(&mut rand::thread_rng()).unwrap_or(0) as i64;

    let next_check = Cron::new(&service.cron_schedule)
        .parse()?
        .find_next_occurrence(&chrono::Utc::now(), false)?
        + chrono::Duration::seconds(jitter);
    model.next_check.set_if_not_equals(next_check);

    if model.is_changed() {
        debug!("Saving {:?}", model);
        model.save(&*db_writer).await.map_err(|err| {
            error!("{} error saving {:?}", service.id.hyphenated(), err);
            Error::from(err)
        })?;
    } else {
        debug!("set_last_check with no change? {:?}", model);
    }

    Ok(())
}

#[instrument(level = "DEBUG", skip_all, fields(service_check_id = %service_check.id, service_id = %service.id))]
async fn run_inner(
    db: Arc<RwLock<DatabaseConnection>>,
    service_check: entities::service_check::Model,
    service: entities::service::Model,
    checks_run_since_startup: Arc<Counter<u64>>,
) -> Result<(), Error> {
    let sc_id = service_check.id.hyphenated().to_string();
    if let Err(err) = run_service_check(db.clone(), &service_check, service).await {
        error!("Failed to run service_check {} error={:?}", sc_id, err);

        let db_writer = db.write().await;
        if let Some(service_check) = entities::service_check::Entity::find()
            .filter(entities::service_check::Column::Id.eq(&sc_id))
            .one(&*db_writer)
            .await
            .map_err(Error::from)?
        {
            let mut service_check = service_check.into_active_model();
            service_check.status.set_if_not_equals(ServiceStatus::Error);
            service_check.update(&*db_writer).await?;
        }

        checks_run_since_startup.add(
            1,
            &[
                KeyValue::new("result", ToString::to_string(&ServiceStatus::Error)),
                KeyValue::new("id", sc_id),
            ],
        );
    } else {
        checks_run_since_startup.add(
            1,
            &[
                KeyValue::new("result", ToString::to_string(&ServiceStatus::Ok)),
                KeyValue::new("id", sc_id),
            ],
        );
    }
    Ok(())
}

#[cfg(not(tarpaulin_include))]
/// Loop around and do the checks, keeping it to a limit based on `max_permits`
pub async fn run_check_loop(
    db: Arc<RwLock<DatabaseConnection>>,
    max_permits: usize,
    metrics_meter: Arc<Meter>,
) -> Result<(), Error> {
    // Create a Counter Instrument.

    use crate::db::get_next_service_check;

    let checks_run_since_startup = metrics_meter
        .u64_counter("checks_run_since_startup")
        .build();
    let checks_run_since_startup = Arc::new(checks_run_since_startup);

    let mut backoff: std::time::Duration = DEFAULT_BACKOFF;
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
                let next_service = get_next_service_check(&*db.read().await).await?;

                if let Some((service_check, service)) = next_service {
                    // set the service_check to running
                    service_check
                        .set_status(ServiceStatus::Checking, db.clone())
                        .await?;
                    tokio::spawn(run_inner(
                        db.clone(),
                        service_check,
                        service,
                        checks_run_since_startup.clone(),
                    ));
                    // we did a thing, so we can reset the back-off time, because there might be another
                    backoff = DEFAULT_BACKOFF;
                } else {
                    // didn't get a task, increase backoff a little, but don't overflow the max
                    backoff += DEFAULT_BACKOFF;
                    if backoff > MAX_BACKOFF {
                        backoff = MAX_BACKOFF;
                    }
                }
                drop(permit); // Release the semaphore when the task is done
            }
            Err(err) => {
                error!("Failed to acquire semaphore permit: {:?}", err);
                // something went wrong so we want to chill a bit
                backoff = std::cmp::max(MAX_BACKOFF / 2, DEFAULT_BACKOFF);
            }
        };
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

        let db_reader = db.read().await;

        let service = entities::service::Entity::find()
            .filter(entities::service::Column::ServiceType.eq(ServiceType::Ping))
            .one(&*db_reader)
            .await
            .expect("Failed to query ping service")
            .expect("Failed to find ping service");

        let service_check = service_check::Entity::find()
            .filter(service_check::Column::ServiceId.eq(service.id))
            .one(&*db_reader)
            .await
            .expect("Failed to query service check")
            .expect("Failed to find service check");
        drop(db_reader);

        run_service_check(db.clone(), &service_check, service)
            .await
            .expect("Failed to run service check");
    }

    #[tokio::test]
    async fn test_run_pending_service_check() {
        let (db, _config) = test_setup().await.expect("Failed to setup test");

        let db_writer = db.write().await;

        service_check::Entity::update_many()
            .col_expr(
                service_check::Column::Status,
                Expr::value(ServiceStatus::Pending),
            )
            .exec(&*db_writer)
            .await
            .expect("Failed to update service checks to pending");

        let service = entities::service::Entity::find()
            .filter(entities::service::Column::ServiceType.eq(ServiceType::Ping))
            .one(&*db_writer)
            .await
            .expect("Failed to query ping service")
            .expect("Failed to find ping service");

        let service_check = service_check::Entity::find()
            .filter(service_check::Column::ServiceId.eq(service.id))
            .one(&*db_writer)
            .await
            .expect("Failed to query service check")
            .expect("Failed to find service check");

        drop(db_writer);
        dbg!(&service, &service_check);

        run_service_check(db.clone(), &service_check, service)
            .await
            .expect("Failed to run service check");
    }
}
