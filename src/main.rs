use clap::Parser;
use maremma::check_loop::run_check_loop;
use maremma::cli::{Actions, CliOpts};
use maremma::prelude::*;
use maremma::web::run_web_server;

use std::process::ExitCode;

#[tokio::main]
#[cfg(not(tarpaulin_include))] // ignore for code coverage
async fn main() -> Result<(), ExitCode> {
    // Logging startup things

    use maremma::setup_logging;
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
            let _db = maremma::db::connect(&config).await.map_err(|err| {
                error!("Failed to start up db: {:?}", err);
                ExitCode::FAILURE
            })?;

            tokio::select! {
                _ = run_check_loop(config.clone()) => {
                    info!("Check loop Finished.");
                },
                _ = run_web_server(config.clone()) => {
                    info!("Web server finished.");
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
