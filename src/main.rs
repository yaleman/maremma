use clap::Parser;
use maremma::cli::{Actions, CliOpts};
use maremma::prelude::*;
use maremma::web::run_web_server;

use maremma::setup_logging;

use std::process::ExitCode;

#[tokio::main]
#[cfg(not(tarpaulin_include))] // ignore for code coverage
async fn main() -> Result<(), ExitCode> {
    use maremma::check_loop::run_check_loop;
    use maremma::db::update_db_from_config;

    let cli = CliOpts::parse();
    if let Err(err) = setup_logging(cli.debug()) {
        println!("Failed to setup logging: {:?}", err);
        return Err(ExitCode::from(1));
    };

    // parse the config file
    let config = Configuration::new(cli.config()).await.map_err(|err| {
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

            update_db_from_config(db.clone(), &config).await?;

            tokio::select! {
                check_loop_result = run_check_loop(config.clone(), db.clone()) => {
                    error!("Check loop bailed: {:?}", check_loop_result);
                },
                web_server_result = run_web_server(config.clone(), db.clone()) => {
                    info!("Web server bailed: {:?}", web_server_result);
                }

            }
        }
        Actions::ShowConfig(_show_config) => {
            // if show_config.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&*config)
                    .unwrap_or(format!("Failed to serialize config: {:?}", &config))
            );
            // } else {
            //     println!("{:#?}", config);
            // }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use maremma::setup_logging;

    #[test]
    fn test_setup_logging() {
        assert!(setup_logging(false).is_ok());
        // it'll throw an error because we're trying to re-init the logger
        assert!(setup_logging(true).is_err());
    }
}
