//! Runs the service checks on a loop

use crate::db::get_next_service_check;
use crate::prelude::*;
use futures::FutureExt;
use opentelemetry::metrics::Counter;
use opentelemetry::KeyValue;
use tokio::task::JoinSet;

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
    db: Arc<DatabaseConnection>,
    service_check: &entities::service_check::Model,
    service: entities::service::Model,
) -> Result<CheckResult, Error> {
    let start_time = chrono::Utc::now();
    let check = match Service::try_from_service_model(&service, db.as_ref()).await {
        Ok(check) => check,
        Err(err) => {
            let errmsg = format!(
                "Failed to convert service check {} to service: {:?}",
                service_check.id, err
            );
            error!(errmsg);
            let result = CheckResult {
                status: ServiceStatus::Error,
                time_elapsed: chrono::Utc::now() - start_time,
                result_text: errmsg,
                ..Default::default()
            };
            entities::service_check_history::Model::from_service_check_result(
                service_check.id,
                &result,
            )
            .into_active_model()
            .insert(db.as_ref())
            .await?;
            entities::service_check::set_check_result(
                service_check.id,
                &service,
                chrono::Utc::now(),
                result.status,
                db.as_ref(),
                0,
            )
            .await?;
            return Ok(result);
        }
    };

    let host: entities::host::Model = match service_check
        .find_related(entities::host::Entity)
        .one(db.as_ref())
        .await?
    {
        Some(host) => host,
        None => {
            let errmsg = format!(
                "Failed to get host for service check: {:?}",
                service_check.id
            );
            error!(errmsg);
            let result = CheckResult {
                status: ServiceStatus::Error,
                time_elapsed: chrono::Utc::now() - start_time,
                result_text: errmsg,
                ..Default::default()
            };
            entities::service_check_history::Model::from_service_check_result(
                service_check.id,
                &result,
            )
            .into_active_model()
            .insert(db.as_ref())
            .await?;
            entities::service_check::set_check_result(
                service_check.id,
                &service,
                chrono::Utc::now(),
                result.status,
                db.as_ref(),
                0,
            )
            .await?;
            return Ok(result);
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

    debug!("Starting service_check={:?}", service_check);
    // Here we actually run the check
    let result = match service_to_run.run(&host).await {
        Ok(val) => val,
        Err(err) => CheckResult {
            status: ServiceStatus::Error,
            time_elapsed: chrono::Utc::now() - start_time,
            result_text: format!("Error: {err:?}"),
            ..Default::default()
        },
    };
    debug!(
        "Completed service_check={:?} result={:?}",
        service_check, result.status
    );
    let max_jitter = service_to_run.jitter_value();

    entities::service_check_history::Model::from_service_check_result(service_check.id, &result)
        .into_active_model()
        .insert(db.as_ref())
        .await?;
    entities::service_check::set_check_result(
        service_check.id,
        &service,
        chrono::Utc::now(),
        result.status,
        db.as_ref(),
        max_jitter,
    )
    .await?;

    Ok(result)
}

#[instrument(level = "DEBUG", skip_all, fields(service_check_id = %service_check.id, service_id = %service.id))]
async fn run_inner(
    db: Arc<DatabaseConnection>,
    service_check: entities::service_check::Model,
    service: entities::service::Model,
    checks_run_since_startup: Arc<Counter<u64>>,
) -> Result<ServiceStatus, Error> {
    let sc_id = service_check.id.hyphenated().to_string();
    match run_service_check(db, &service_check, service).await {
        Err(err) => {
            error!("Failed to run service_check {} error={:?}", sc_id, err);
            checks_run_since_startup.add(
                1,
                &[
                    KeyValue::new("result", ToString::to_string(&ServiceStatus::Error)),
                    KeyValue::new("id", sc_id),
                ],
            );
            Err(err)
        }
        Ok(result) => {
            checks_run_since_startup.add(
                1,
                &[
                    KeyValue::new("result", ToString::to_string(&result.status)),
                    KeyValue::new("id", sc_id),
                ],
            );
            Ok(result.status)
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CheckTaskOutcome {
    service_check_id: Uuid,
    status: ServiceStatus,
}

async fn run_supervised_check(
    db: Arc<DatabaseConnection>,
    service_check: entities::service_check::Model,
    service: entities::service::Model,
    checks_run_since_startup: Arc<Counter<u64>>,
) -> CheckTaskOutcome {
    let service_check_id = service_check.id;

    match std::panic::AssertUnwindSafe(run_inner(
        db.clone(),
        service_check.clone(),
        service,
        checks_run_since_startup,
    ))
    .catch_unwind()
    .await
    {
        Ok(Ok(status)) => CheckTaskOutcome {
            service_check_id,
            status,
        },
        Ok(Err(err)) => {
            error!(
                "Failed to supervise service_check {} error={:?}",
                service_check_id, err
            );
            if let Err(set_status_err) = service_check
                .set_status(ServiceStatus::Error, db.as_ref())
                .await
            {
                error!(
                    "Failed to persist error status for service_check {}: {:?}",
                    service_check_id, set_status_err
                );
            }
            CheckTaskOutcome {
                service_check_id,
                status: ServiceStatus::Error,
            }
        }
        Err(_panic) => {
            error!("Service check task panicked for {}", service_check_id);
            if let Err(set_status_err) = service_check
                .set_status(ServiceStatus::Error, db.as_ref())
                .await
            {
                error!(
                    "Failed to persist panic status for service_check {}: {:?}",
                    service_check_id, set_status_err
                );
            }
            CheckTaskOutcome {
                service_check_id,
                status: ServiceStatus::Error,
            }
        }
    }
}

#[cfg(not(tarpaulin_include))]
/// Loop around and do the checks, keeping it to a limit based on `max_permits`
pub async fn run_check_loop(
    db: Arc<DatabaseConnection>,
    max_permits: usize,
    metrics_meter: Arc<Meter>,
) -> Result<(), Error> {
    // Create a Counter Instrument.

    use std::cmp::min;

    let max_permits = std::cmp::max(max_permits, 1);
    let checks_run_since_startup = metrics_meter
        .u64_counter("checks_run_since_startup")
        .build();
    let checks_run_since_startup = Arc::new(checks_run_since_startup);

    let mut backoff: std::time::Duration = DEFAULT_BACKOFF;
    info!("Max concurrent tasks set to {}", max_permits);
    let mut tasks: JoinSet<CheckTaskOutcome> = JoinSet::new();

    loop {
        while let Some(join_result) = tasks.try_join_next() {
            match join_result {
                Ok(outcome) => {
                    debug!(
                        "Completed supervised service_check {} with status {}",
                        outcome.service_check_id, outcome.status
                    );
                }
                Err(err) => {
                    error!("Service check task exited unexpectedly: {:?}", err);
                }
            }
        }

        while tasks.len() >= max_permits {
            warn!("No spare task slots, something might be running slow!");
            match tasks.join_next().await {
                Some(Ok(outcome)) => {
                    debug!(
                        "Completed supervised service_check {} with status {}",
                        outcome.service_check_id, outcome.status
                    );
                }
                Some(Err(err)) => error!("Service check task exited unexpectedly: {:?}", err),
                None => break,
            }
        }

        match get_next_service_check(db.as_ref()).await? {
            Some((service_check, service)) => {
                service_check
                    .set_status(ServiceStatus::Checking, db.as_ref())
                    .await?;
                tasks.spawn(run_supervised_check(
                    db.clone(),
                    service_check,
                    service,
                    checks_run_since_startup.clone(),
                ));
                backoff = DEFAULT_BACKOFF;
            }
            None => {
                backoff = min(MAX_BACKOFF, backoff + DEFAULT_BACKOFF);
                if tasks.is_empty() {
                    tokio::time::sleep(backoff).await;
                } else {
                    tokio::select! {
                        Some(join_result) = tasks.join_next() => {
                            match join_result {
                                Ok(outcome) => {
                                    debug!(
                                        "Completed supervised service_check {} with status {}",
                                        outcome.service_check_id, outcome.status
                                    );
                                }
                                Err(err) => error!("Service check task exited unexpectedly: {:?}", err),
                            }
                        }
                        _ = tokio::time::sleep(backoff) => {}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use entities::service_check;
    use opentelemetry::metrics::MeterProvider;
    use sea_orm::{ActiveModelTrait, EntityTrait, QueryFilter, Set};

    use super::*;
    use crate::db::entities::{host, service};
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
        let initial_last_updated = service_check.last_updated;

        let result = run_service_check(db.clone(), &service_check, service)
            .await
            .expect("Failed to run service check");

        let updated_check = service_check::Entity::find_by_id(service_check.id)
            .one(db.as_ref())
            .await
            .expect("Failed to reload service check")
            .expect("Failed to find updated service check");

        let history = entities::service_check_history::Entity::find()
            .filter(entities::service_check_history::Column::ServiceCheckId.eq(service_check.id))
            .all(db.as_ref())
            .await
            .expect("Failed to query service check history");

        assert_eq!(updated_check.status, result.status);
        assert!(updated_check.last_updated > initial_last_updated);
        assert!(updated_check.next_check > chrono::Utc::now());
        assert!(!history.is_empty());
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

        run_service_check(db.clone(), &service_check, service)
            .await
            .expect("Failed to run service check");
    }

    #[tokio::test]
    async fn test_run_check_loop_respects_max_concurrency() {
        let db = Arc::new(
            crate::db::test_connect()
                .await
                .expect("Failed to connect to test DB"),
        );
        let sleep_service = service::Model {
            id: Uuid::new_v4(),
            name: "sleep-check".to_string(),
            description: None,
            service_type: ServiceType::Cli,
            cron_schedule: "* * * * *".to_string(),
            extra_config: json!({"command_line": "sleep 1"}),
        };
        let first_host = host::Model {
            id: Uuid::new_v4(),
            name: "sleep-host-1".to_string(),
            hostname: "localhost".to_string(),
            check: crate::host::HostCheck::None,
            config: json!({}),
        };
        let second_host = host::Model {
            id: Uuid::new_v4(),
            name: "sleep-host-2".to_string(),
            hostname: "localhost".to_string(),
            check: crate::host::HostCheck::None,
            config: json!({}),
        };

        service::Entity::insert(sleep_service.clone().into_active_model())
            .exec(db.as_ref())
            .await
            .expect("Failed to insert test service");
        host::Entity::insert(first_host.clone().into_active_model())
            .exec(db.as_ref())
            .await
            .expect("Failed to insert first host");
        host::Entity::insert(second_host.clone().into_active_model())
            .exec(db.as_ref())
            .await
            .expect("Failed to insert second host");

        for host_id in [first_host.id, second_host.id] {
            service_check::ActiveModel {
                id: Set(Uuid::new_v4()),
                service_id: Set(sleep_service.id),
                host_id: Set(host_id),
                status: Set(ServiceStatus::Pending),
                last_check: Set(chrono::Utc::now() - chrono::Duration::minutes(1)),
                next_check: Set(chrono::Utc::now() - chrono::Duration::minutes(1)),
                last_updated: Set(chrono::Utc::now() - chrono::Duration::minutes(1)),
            }
            .insert(db.as_ref())
            .await
            .expect("Failed to insert test service_check");
        }

        let (provider, _registry) = crate::metrics::new().expect("Failed to create metrics");
        let runner = tokio::spawn(run_check_loop(
            db.clone(),
            1,
            Arc::new(provider.meter("maremma-test")),
        ));

        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let checking_count = service_check::Entity::find()
            .filter(service_check::Column::Status.eq(ServiceStatus::Checking))
            .all(db.as_ref())
            .await
            .expect("Failed to query checking service checks")
            .len();

        runner.abort();
        let _ = runner.await;

        assert_eq!(checking_count, 1);
    }
}
