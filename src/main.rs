use clap::Parser;
use env_logger::{Builder, Target};
use maremma::cli::{Actions, CliOpts};
use maremma::prelude::*;
use std::env;
use std::process::ExitCode;

fn setup_logging(debug: bool) -> Result<(), log::SetLoggerError> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    let mut builder = Builder::from_default_env();
    let builder = if debug {
        builder.filter_level(tracing::log::LevelFilter::Debug)
    } else {
        &mut builder
    };
    builder.target(Target::Stdout);
    builder.try_init()
}

#[tokio::main]
#[cfg(not(tarpaulin_include))] // ignore for code coverage
async fn main() -> Result<(), ExitCode> {
    // Logging startup things

    let cli = CliOpts::parse();
    setup_logging(cli.debug()).expect("Failed to start logging!");

    // parse the config file
    let config = Configuration::new(cli.config()).await.map_err(|err| {
        error!("Failed to load config: {:?}", err);
        ExitCode::from(1)
    })?;

    match cli.action {
        Actions::Run(_) => run_check_loop(config).await,
        Actions::ShowConfig(show_config) => {
            if show_config.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&config).expect("Failed to serialize config!")
                );
            } else {
                println!("{:#?}", config);
            }
        }
    }
    Ok(())
}

#[cfg(not(tarpaulin_include))] // ignore for code coverage
async fn run_check_loop(config: Configuration) {
    info!("Starting up!");

    loop {
        match config.get_next_service_check().await {
            Some(next_check_id) => {
                match config.run_check(&next_check_id).await {
                    Ok((hostname, status)) => {
                        let service_id_reader = config.service_checks.read().await;
                        let service_id: String = service_id_reader
                            .get(&next_check_id)
                            .map(|s| s.service_id.clone())
                            .expect("Service not found after doing check?");
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

#[cfg(test)]
mod tests {
    use crate::setup_logging;

    #[test]
    fn test_setup_logging() {
        assert!(setup_logging(false).is_ok());
        // it'll throw an error because we're trying to re-init the logger
        assert!(setup_logging(true).is_err());
    }
}
