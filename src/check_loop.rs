use crate::prelude::*;

#[cfg(not(tarpaulin_include))] // ignore for code coverage
pub async fn run_check_loop(config: Arc<Configuration>) {
    info!("Starting up!");

    loop {
        match config.get_next_service_check().await {
            Some(next_check_id) => {
                match config.run_check(&next_check_id).await {
                    Ok((hostname, status)) => {
                        let service_id_reader = config.service_checks.read().await;
                        let service_id: String = match service_id_reader
                            .get(&next_check_id)
                            .map(|s| s.service_id.clone())
                        {
                            Some(val) => val,
                            None => {
                                error!(
                                    "Failed to get service_id from next_check_id: {:?}",
                                    next_check_id
                                );
                                drop(service_id_reader);
                                continue;
                            }
                        };

                        drop(service_id_reader);

                        let service = match config.get_service(&service_id) {
                            Some(service) => service,
                            None => {
                                error!("Failed to get service ID: {:?}", service_id);
                                continue;
                            }
                        };

                        status.log(&format!(
                            "{next_check_id} {hostname} {} {:?}",
                            service.name, &status
                        ));

                        debug!("Checking in service check... {}", &next_check_id);
                        if let Some(service_check) =
                            config.service_checks.write().await.get_mut(&next_check_id)
                        {
                            service_check.checkin(status);
                        } else {
                            error!("Failed to check in service check: {}", next_check_id);
                        }
                    }
                    Err(err) => {
                        error!("{} Failed to run check: {:?}", next_check_id, err);
                    }
                };
            }
            None => {
                let next_wakeup = config.find_next_wakeup().await;

                let delta = next_wakeup - chrono::Utc::now();
                if delta.num_microseconds().unwrap_or(0) > 0 {
                    debug!(
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
}
