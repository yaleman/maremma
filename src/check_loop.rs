//! Runs the service checks on a loop

use crate::prelude::*;
use mpsc::Sender;
use opentelemetry::metrics::Counter;
use opentelemetry::KeyValue;
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

#[instrument(level = "INFO", skip(tx))]
/// Does what it says on the tin
pub(crate) async fn run_service_check(
    tx: mpsc::Sender<DbActorMessage>,
    service_check: &entities::service_check::Model,
    service: entities::service::Model,
) -> Result<(), Error> {
    let (sender, run_rx) = oneshot::channel();
    let msg = DbActorMessage::GetRunnableCheck {
        service_check: service_check.clone(),
        service: service.clone(),
        sender,
    };
    debug!("Sending message");
    tx.send(msg).await.map_err(Error::from)?;
    let (host, check) = run_rx.await?;

    #[cfg(not(tarpaulin_include))]
    let service_to_run = check.config().ok_or_else(|| {
        error!("Failed to get service config for {}", check.id.hyphenated());
        Error::ServiceConfigNotFound(check.id.hyphenated().to_string())
    })?;

    debug!("starting service_check={:?}", service_check);
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
        "done service_check={:?} result={:?}",
        service_check, result.status
    );

    let msg = DbActorMessage::SetCheckResult {
        service_check: service_check.clone(),
        service: service.clone(),
        last_check: chrono::Utc::now(),
        result,
        jitter,
    };

    if let Err(err) = tx.send(msg).await {
        // TODO: better error
        error!("Failed to send service check result error={}", err);
    }
    Ok(())
}

#[instrument(level = "DEBUG", skip_all, fields(service_check_id = %service_check.id, service_id = %service.id))]
async fn run_inner(
    tx: Sender<DbActorMessage>,
    service_check: entities::service_check::Model,
    service: entities::service::Model,
    checks_run_since_startup: Arc<Counter<u64>>,
) -> Result<(), Error> {
    let sc_id = service_check.id.hyphenated().to_string();
    if let Err(err) = run_service_check(tx.clone(), &service_check, service).await {
        error!("Failed to run service_check {} error={:?}", sc_id, err);

        let (sender, rx) = oneshot::channel();
        drop(rx);

        let msg = DbActorMessage::SetStatus {
            service_check_id: service_check.id,
            status: ServiceStatus::Error,
            sender,
        };

        if let Err(err) = tx.send(msg).await {
            error!(
                "Failed to send set_status message for service_check={}  {:?}",
                sc_id, err
            );
        } else {
            debug!("Sent set_status message for service_check={}", sc_id);
        };

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
    tx: mpsc::Sender<DbActorMessage>,
    max_permits: usize,
    metrics_meter: Arc<Meter>,
) -> Result<(), Error> {
    // Create a Counter Instrument.

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
                let (actor_tx, responder) = oneshot::channel();
                tx.send(DbActorMessage::NextServiceCheck(actor_tx)).await?;

                if let Some((service_check, service)) = responder.await? {
                    tokio::spawn(run_inner(
                        tx.clone(),
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
        let (db, _config, mut dbactor, tx) = test_setup().await.expect("Failed to setup test");

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

        tokio::select! {
            _ = dbactor.run_actor() => {},
            _ = run_service_check(tx, &service_check, service) => {}
        };
    }

    #[tokio::test]
    async fn test_run_pending_service_check() {
        let (db, _config, mut dbactor, tx) = test_setup().await.expect("Failed to setup test");

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

        tokio::select! {
            _ = dbactor.run_actor() => {},
            res = run_service_check(tx, &service_check, service) => {
                res.expect("Failed to run service check");

            }
        };
    }
}
