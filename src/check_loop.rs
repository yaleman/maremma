//! Runs the service checks on a loop

use crate::db::get_next_service_check;
use crate::prelude::*;
use opentelemetry::metrics::Counter;
use opentelemetry::KeyValue;
use rand::seq::IteratorRandom;

use std::cmp::min;

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

impl Default for CheckResult {
    fn default() -> Self {
        Self {
            timestamp: chrono::Utc::now(),
            time_elapsed: Duration::zero(),
            status: ServiceStatus::Unknown,
            result_text: String::new(),
        }
    }
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
            let errmsg = format!(
                "Failed to convert service check {} to service: {:?}",
                service_check.id, err
            );
            error!(errmsg);
            return Err(Error::Generic(errmsg));
        }
    };

    let host: entities::host::Model = match service_check
        .find_related(entities::host::Entity)
        .one(&*db_writer)
        .await
        .inspect_err(|err| {
            error!(
                "Failed to search for host for service_check={} error={}",
                service_check.id, err
            )
        })? {
        Some(host) => host,
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
    let start_time = chrono::Utc::now();
    // Here we actually run the check
    let result = match service_to_run.run(&host).await {
        Ok(val) => val,
        Err(err) => CheckResult {
            status: ServiceStatus::Error,
            time_elapsed: chrono::Utc::now() - start_time,
            result_text: format!("Error: {:?}", err),
            ..Default::default()
        },
    };
    debug!(
        "Completed service_check={:?} result={:?}",
        service_check.id, result.status
    );
    let max_jitter = service_to_run.jitter_value();

    let db_writer = db.write().await;
    entities::service_check_history::Model::from_service_check_result(service_check.id, &result)
        .into_active_model()
        .insert(&*db_writer)
        .await
        .inspect_err(|err| {
            error!(
                "Failed to insert service check history for {}: {:?}",
                service_check.id, err
            )
        })?;

    let mut model = service_check.clone().into_active_model();
    model.last_check.set_if_not_equals(chrono::Utc::now());
    model.status.set_if_not_equals(result.status);

    // get a number between 0 and jitter
    let jitter: i64 = (0..max_jitter).choose(&mut rand::rng()).unwrap_or(0) as i64;

    let next_check: DateTime<Utc> = Cron::new(&service.cron_schedule)
        .parse()
        .inspect_err(|err| {
            error!(
                "Failed to parse cron schedule while setting next occurrence of {:?}: {:?}",
                model.id, err
            );
        })?
        .find_next_occurrence(&chrono::Utc::now(), false)
        .inspect_err(|err| {
            error!(
                "Failed to get next occurrence for check {}! {err:?}",
                service_check.id
            )
        })?
        + chrono::Duration::seconds(jitter);
    model.next_check.set_if_not_equals(next_check);

    model.update(&*db_writer).await.inspect_err(|err| {
        error!("{} error saving {:?}", service.id.hyphenated(), err);
    })?;

    drop(db_writer);
    debug!(
        "run_service_check service_check={} completed",
        service_check.id
    );

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
    match run_service_check(db.clone(), &service_check, service).await {
        Err(err) => {
            error!("Failed to run service_check {} error={:?}", sc_id, err);

            let db_writer: tokio::sync::RwLockWriteGuard<'_, DatabaseConnection> = db.write().await;
            match entities::service_check::Entity::find()
                .filter(entities::service_check::Column::Id.eq(&sc_id))
                .one(&*db_writer)
                .await
                .map_err(Error::from)?
            {
                Some(service_check) => {
                    service_check
                        .set_status(ServiceStatus::Error, &db_writer)
                        .await?;
                }
                _ => {
                    error!(
                        "Trying to set error status but couldn't find service check {}",
                        sc_id
                    );
                }
            }
            drop(db_writer);

            checks_run_since_startup.add(
                1,
                &[
                    KeyValue::new("result", ToString::to_string(&ServiceStatus::Error)),
                    KeyValue::new("id", sc_id),
                ],
            );
        }
        Ok(_) => {
            checks_run_since_startup.add(
                1,
                &[
                    KeyValue::new("result", ToString::to_string(&ServiceStatus::Ok)),
                    KeyValue::new("id", sc_id),
                ],
            );
        }
    };

    Ok(())
}

#[cfg(not(tarpaulin_include))]
/// Loop around and do the checks, keeping it to a limit based on `max_permits`
pub async fn run_check_loop(
    db: Arc<RwLock<DatabaseConnection>>,
    max_tasks: usize,
    metrics_meter: Arc<Meter>,
) -> Result<(), Error> {
    // Create a Counter Instrument.

    let checks_run_since_startup = metrics_meter
        .u64_counter("checks_run_since_startup")
        .build();
    let checks_run_since_startup = Arc::new(checks_run_since_startup);

    let mut backoff: std::time::Duration = DEFAULT_BACKOFF;
    // Limit to n concurrent tasks
    info!("Max concurrent tasks set to {}", max_tasks);

    loop {
        let mut tasks = Vec::new();
        while tasks.len() < max_tasks {
            let db_lock = db.write().await;
            let next_service = match get_next_service_check(&db_lock).await {
                Ok(val) => val,
                Err(err) => {
                    if let Error::SqlError(DbErr::ConnectionAcquire(ConnAcquireErr::Timeout)) = err
                    {
                        error!(
                            "Timed out trying to acquire connection, retrying in {:?}",
                            backoff
                        );
                        drop(db_lock);
                        tokio::time::sleep(backoff).await;
                        continue;
                    } else {
                        error!("Failed to get next service check: {:?}", err);
                        return Err(err);
                    }
                }
            };

            match next_service {
                Some((service_check, service)) => {
                    // set the service_check to running
                    if let Err(err) = service_check
                        .set_status(ServiceStatus::Checking, &db_lock)
                        .await
                    {
                        if let Error::SqlError(DbErr::ConnectionAcquire(ConnAcquireErr::Timeout)) =
                            err
                        {
                            error!(
                                "Timed out trying to acquire connection, retrying in {:?}",
                                backoff
                            );
                            drop(db_lock);
                            tokio::time::sleep(backoff).await;
                            continue;
                        } else {
                            error!("Failed to get next service check: {:?}", err);
                            return Err(err);
                        }
                    };
                    drop(db_lock);
                    tasks.push(tokio::spawn(run_inner(
                        db.clone(),
                        service_check,
                        service,
                        checks_run_since_startup.clone(),
                    )));
                    // we did a thing, so we can reset the back-off time, because there might be another
                    backoff = DEFAULT_BACKOFF;
                }
                None => {
                    drop(db_lock);
                    // didn't get a task, increase backoff a little, but don't overflow the max
                    backoff = min(MAX_BACKOFF, backoff + DEFAULT_BACKOFF);
                }
            };
        }
        // we're at max tasks, so we need to wait for one to finish
        for res in futures::future::join_all(tasks).await {
            if let Err(err) = res {
                error!("Error running check: {:?}", err);
            }
        }
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

        let db_lock = db.write().await;

        let service = entities::service::Entity::find()
            .filter(entities::service::Column::ServiceType.eq(ServiceType::Ping))
            .one(&*db_lock)
            .await
            .expect("Failed to query ping service")
            .expect("Failed to find ping service");

        let service_check = service_check::Entity::find()
            .filter(service_check::Column::ServiceId.eq(service.id))
            .one(&*db_lock)
            .await
            .expect("Failed to query service check")
            .expect("Failed to find service check");
        drop(db_lock);

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
