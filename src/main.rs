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

#[cfg(not(tarpaulin_include))] // ignore for code coverage
fn main() -> Result<(), ExitCode> {
    let cli = CliOpts::parse();
    if let Err(err) = setup_logging(cli.debug(), cli.db_debug(), cli.tokio_console()) {
        eprintln!("Failed to setup logging: {err:?}");
        return Err(ExitCode::from(1));
    };

    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let num_cpus = num_cpus::get();
    let threads = std::cmp::min(4, num_cpus);
    debug!("Starting {} threads", threads);
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(threads)
        .enable_all()
        .build()
        .unwrap()
        .block_on(async_main(cli))
}

#[cfg(not(tarpaulin_include))] // ignore for code coverage
async fn async_main(cli: CliOpts) -> Result<(), ExitCode> {
    use maremma::db::get_connect_string;
    use maremma::services::oneshot::run_oneshot;
    use maremma::shepherd::shepherd;

    if let Actions::ExportConfigSchema = cli.action {
        let schema = schemars::schema_for!(Configuration);
        println!("{}", serde_json::to_string_pretty(&schema).unwrap());
        return Ok(());
    }

    // parse the config file
    let config = Configuration::new(&cli.config()).await.map_err(|err| {
        error!("Failed to load config: {:?}", err);
        ExitCode::from(1)
    })?;

    let config = Arc::new(RwLock::new(config));

    // in case we need it, get the connect string
    let connect_string = get_connect_string(config.clone()).await;
    let db = Arc::new(RwLock::new(
        maremma::db::connect(config.clone()).await.map_err(|err| {
            error!("Failed to start up db from '{}' {:?}", connect_string, err);
            ExitCode::FAILURE
        })?,
    ));

    match cli.action {
        Actions::Run(_) => {
            if update_db_from_config(&*db.write().await, config.clone())
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

            let (web_tx, web_rx) = tokio::sync::mpsc::channel(1);

            tokio::select! {

                check_loop_result = run_check_loop(
                    db.clone(),
                    config.read().await.max_concurrent_checks,
                    metrics_meter.clone()
                ) => {
                    error!("Check loop bailed: {:?}", check_loop_result);
                },
                web_server_result = run_web_server(
                    cli.config(),
                    config.clone(),
                    db.clone(),
                    Arc::new(registry),
                    web_tx.clone(),
                    web_rx,
                ) => {
                    error!("Web server bailed: {:?}", web_server_result);
                },
                shepherd_result = shepherd(db.clone(), config.clone(), web_tx) => {
                    error!("Shepherd bailed: {:?}", shepherd_result);
                }

            }
        }
        Actions::CheckConfig(_show_config) => {
            todo!("Check config CLI hasn't been implemented")
        }
        Actions::ShowConfig(_show_config) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&*config.read().await)
                    .unwrap_or(format!("Failed to serialize config: {:?}", &config))
            );
        }
        Actions::OneShot(cmd) => match run_oneshot(cmd, config).await {
            Err(maremma::errors::Error::OneShotFailed) => return Err(ExitCode::from(1)),
            Err(err) => error!("Failed to run oneshot: {:?}", err),
            Ok(_) => {}
        },
        Actions::ExportConfigSchema => unreachable!(),
    }
    Ok(())
}
