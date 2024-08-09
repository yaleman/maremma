use crate::db::get_next_service_check;
use crate::prelude::*;
use chrono::Duration;
use sea_orm::prelude::*;
use tokio::sync::Semaphore;

#[derive(Clone, Debug)]
#[allow(dead_code)] // 'cause debug
pub struct CheckResult {
    pub timestamp: chrono::DateTime<Utc>,
    pub time_elapsed: Duration,
    pub status: ServiceStatus,
    pub result_text: String,
}

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
            if let Err(err) = service_check
                .set_status(ServiceStatus::Error, db.as_ref())
                .await
            {
                return Err(Error::Generic(format!(
                    "Failed to set 'error' status for service_check_id={} error={:?}",
                    service_check.id.hyphenated(),
                    err
                )));
            };
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

    info!("id={} result={:?}", service_check.id, result);
    service_check
        .set_last_check(&service, chrono::Utc::now(), result.status, db.as_ref())
        .await
        .map_err(|err| {
            error!(
                "Failed to set status for service check: {:?}",
                service_check.id
            );
            err
        })?;

    entities::service_check_history::Model::from_service_check_result(service_check.id, &result)
        .into_active_model()
        .insert(db.as_ref())
        .await?;

    Ok(())
}

#[cfg(not(tarpaulin_include))] // TODO: tarpaulin un-ignore for code coverage
pub async fn run_check_loop(db: Arc<DatabaseConnection>, max_permits: usize) -> Result<(), Error> {
    let mut backoff = tokio::time::Duration::from_millis(50);
    let semaphore = Arc::new(Semaphore::new(max_permits)); // Limit to n concurrent tasks
    info!("Max concurrent tasks set to {}", max_permits);
    loop {
        if let Some((service_check, service)) = get_next_service_check(db.as_ref()).await? {
            service_check
                .set_status(ServiceStatus::Checking, db.as_ref())
                .await?;

            match semaphore.clone().acquire_owned().await {
                Ok(permit) => {
                    let db_clone = db.clone();
                    tokio::spawn(async move {
                        if let Err(err) = run_service_check(db_clone, service_check, service).await
                        {
                            error!("Failed to run service check: {:?}", err);
                        }
                        drop(permit); // Release the permit when the task is done
                    });
                }
                Err(err) => {
                    error!("Failed to acquire semaphore permit: {:?}", err);
                }
            };
            backoff = tokio::time::Duration::from_millis(50);
        } else {
            backoff += tokio::time::Duration::from_millis(50);
            if backoff > tokio::time::Duration::from_secs(1) {
                backoff = tokio::time::Duration::from_secs(1);
            }
            debug!("Nothing to do, waiting {}ms", backoff.as_millis());
            tokio::time::sleep(backoff).await;
        }
        if semaphore.available_permits() == 0 {
            warn!("No spare task slots, something might be running slow!");
        }
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
