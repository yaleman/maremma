use chrono::TimeDelta;
use env_logger::{Builder, Target};
use maremma::prelude::*;
use maremma::services::check::{service_check_id, ServiceCheck};
use std::env;

#[tokio::main]
async fn main() -> Result<(), ()> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    let mut builder = Builder::from_default_env();
    builder.target(Target::Stdout);
    builder.init();

    // parse the config file
    let config = Configuration::new("config.json").map_err(|err| {
        error!("Failed to load config: {:?}", err);
    })?;

    debug!(
        "Config:\n{}",
        serde_json::to_string_pretty(&config).expect("Failed to serialize config!")
    );

    let mut service_checks: ServiceChecks = Arc::new(RwLock::new(HashMap::new()));
    update_service_checks(&mut service_checks, &config).await;

    info!("Starting up!");

    loop {
        if let Some(next_check_id) = get_next_service_check(&mut service_checks, &config).await {
            debug!("Next check: {}", next_check_id);

            match run_check(&service_checks, &config, &next_check_id).await {
                Ok((hostname, status)) => {
                    match status {
                        ServiceStatus::Ok | ServiceStatus::Checking => {
                            info!("{} {} Status: {:?}", next_check_id, hostname, status)
                        }
                        ServiceStatus::Unknown
                        | ServiceStatus::Urgent
                        | ServiceStatus::Pending
                        | ServiceStatus::Warning => {
                            warn!("{} {} Status: {:?}", next_check_id, hostname, status)
                        }
                        ServiceStatus::Critical | ServiceStatus::Error => {
                            error!("{} {} Status: {:?}", next_check_id, hostname, status)
                        }
                    };
                    if let Some(service_check) =
                        service_checks.write().await.get_mut(&next_check_id)
                    {
                        service_check.checkin(status);
                    }
                }
                Err(err) => {
                    error!("{} Failed to run check: {:?}", next_check_id, err);
                }
            };
        } else {
            let next_wakeup = find_next_wakeup(&mut service_checks, &config).await;

            let delta = next_wakeup - chrono::Utc::now();
            if delta.num_seconds() > 0 {
                info!(
                    "No checks to run, sleeping for {} seconds",
                    delta.num_seconds()
                );
                tokio::time::sleep(core::time::Duration::from_millis(
                    delta.num_milliseconds() as u64
                ))
                .await;
            }
        }
    }
}

async fn run_check(
    service_checks: &ServiceChecks,
    config: &Configuration,
    next_check_id: &str,
) -> Result<(String, ServiceStatus), Error> {
    let check = service_checks.read().await;
    let check = check
        .get(next_check_id)
        .ok_or(Error::ServiceCheckNotFound(next_check_id.to_string()))?;

    let host = config
        .hosts
        .iter()
        .find(|host| host.host_id() == check.host_id)
        .ok_or(Error::HostNotFound((*check.host_id).clone()))?;

    let service = config
        .service_table
        .get(&check.service_id)
        .ok_or(Error::ServiceNotFound)?;
    if let Some(config) = &service.config {
        match config.run(host).await {
            Ok(val) => Ok((host.hostname(), val)),
            Err(err) => Err(err),
        }
    } else {
        Err(Error::ServiceConfigNotFound(next_check_id.to_string()))
    }
}

async fn update_service_checks(service_checks: &mut ServiceChecks, configuration: &Configuration) {
    for (host_group_id, service_ids) in &configuration.host_group_services {
        for service_id in service_ids {
            for host_id in configuration
                .host_group_members
                .get(host_group_id)
                .cloned()
                .unwrap_or(vec![])
            {
                let service_check_id = service_check_id(host_id.clone(), service_id);
                // check if the servicecheck exists already

                if let std::collections::hash_map::Entry::Vacant(e) =
                    service_checks.write().await.entry(service_check_id.clone())
                {
                    debug!(
                        "Adding service check: {} to host: {}",
                        service_check_id, host_id
                    );
                    // create a new service check
                    e.insert(ServiceCheck::new(host_id.clone(), service_id.clone()));
                } else {
                    // TODO: update the service check
                    debug!("Service check: {} already exists", service_check_id);
                }
            }
        }
    }
}

async fn find_next_wakeup(
    service_checks: &mut ServiceChecks,
    config: &Configuration,
) -> DateTime<Utc> {
    let mut next_wakeup: Option<DateTime<Utc>> = None;

    // find the next time we need to wake up
    let one_sec = TimeDelta::new(1, 0).expect("Failed to get a 1 second TimeDelta");
    for (_id, check) in service_checks.read().await.iter() {
        if let Ok(cron) = check.get_cron(config) {
            if let Ok(next_runtime) = cron.find_next_occurrence(&check.last_check, true) {
                match next_wakeup {
                    Some(wakeup) => {
                        if next_runtime < wakeup {
                            next_wakeup = Some(next_runtime);
                        }
                    }
                    None => {
                        next_wakeup = Some(next_runtime);
                    }
                }
            }
        }
    }
    next_wakeup.unwrap_or(chrono::Utc::now() + one_sec)
}

/// Get the next service check to run
async fn get_next_service_check(
    service_checks: &mut ServiceChecks,
    config: &Configuration,
) -> Option<String> {
    // Try and get an urgent one first
    if let Some(id) = service_checks
        .write()
        .await
        .iter_mut()
        .find_map(|(id, check)| {
            if let ServiceStatus::Urgent = check.status {
                check.checkout();
                return Some(id.to_owned());
            }
            None
        })
    {
        return Some(id);
    }
    let now = Some(chrono::Utc::now());

    service_checks
        .write()
        .await
        .iter_mut()
        .find_map(|(id, check)| {
            if let ServiceStatus::Checking = check.status {
                // we're already checking this
                return None;
            }

            if check.is_due(config, now).unwrap_or(false) {
                debug!("Returning {}", check.check_id());
                check.checkout();
                Some(id.to_owned())
            } else {
                None
            }
        })
}
