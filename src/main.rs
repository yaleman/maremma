use clap::Parser;
use maremma::cli::{Actions, CliOpts};
use maremma::config::Configuration;
use maremma::prelude::*;
use maremma::web::run_web_server;

use maremma::log::setup_logging;

use maremma::check_loop::run_check_loop;
use maremma::db::update_db_from_config;
use opentelemetry::metrics::MeterProvider;
use std::process::ExitCode;

#[tokio::main]
#[cfg(not(tarpaulin_include))] // ignore for code coverage
async fn main() -> Result<(), ExitCode> {
    use maremma::services::oneshot::run_oneshot;
    use maremma::shepherd::shepherd;

    let cli = CliOpts::parse();
    if let Err(err) = setup_logging(cli.debug(), cli.db_debug()) {
        println!("Failed to setup logging: {:?}", err);
        return Err(ExitCode::from(1));
    };

    // parse the config file
    let config = Configuration::new(&cli.config()).await.map_err(|err| {
        error!("Failed to load config: {:?}", err);
        ExitCode::from(1)
    })?;

    let config = Arc::new(config);

    match cli.action {
        Actions::Run(_) => {
            let db = Arc::new(maremma::db::connect(&config).await.map_err(|err| {
                error!("Failed to start up db: {:?}", err);
                ExitCode::FAILURE
            })?);

            if update_db_from_config(db.clone(), config.clone())
                .await
                .is_err()
            {
                return Err(ExitCode::FAILURE);
            };

            // start up the metrics provider
            let (provider, registry) = maremma::metrics::new().map_err(|err| {
                error!("Failed to start metrics Provider: {:?}", err);
                ExitCode::FAILURE
            })?;

            // Create a meter from the above MeterProvider.
            let metrics_meter = Arc::new(provider.meter("maremma"));

            tokio::select! {
                check_loop_result = run_check_loop(db.clone(), config.max_concurrent_checks, metrics_meter.clone()) => {
                    error!("Check loop bailed: {:?}", check_loop_result);
                },
                web_server_result = run_web_server(config.clone(), db.clone(), Arc::new(registry)) => {
                    error!("Web server bailed: {:?}", web_server_result);
                },
                shepherd_result = shepherd(db.clone(), config.clone()) => {
                    error!("Shepherd bailed: {:?}", shepherd_result);
                }

            }
        }
        Actions::CheckConfig(_show_config) => {
            todo!()
        }
        Actions::ShowConfig(_show_config) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&*config)
                    .unwrap_or(format!("Failed to serialize config: {:?}", &config))
            );
        }
        Actions::OneShot(cmd) => match run_oneshot(cmd, config).await {
            Err(maremma::errors::Error::OneShotFailed) => return Err(ExitCode::from(1)),
            Err(err) => error!("Failed to run oneshot: {:?}", err),
            Ok(_) => {}
        },

        Actions::ExportConfigSchema => {
            let schema = schemars::schema_for!(Configuration);
            println!("{}", serde_json::to_string_pretty(&schema).unwrap());
        }
    }
    Ok(())
}
