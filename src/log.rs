//! log configuration and setup module

use std::env;

use env_logger::{Builder, Target};
use log::LevelFilter;

/// Sets up logging
pub fn setup_logging(
    config_log_level: Option<String>,
    debug: bool,
    db_debug: bool,
    tokio_console: bool,
) -> Result<(), log::SetLoggerError> {
    // check the env vars
    #[cfg(not(any(debug_assertions, test)))]
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }

    let mut filters: Vec<(&str, LevelFilter)> = vec![
        ("ssh::client", LevelFilter::Warn),
        ("ssh::channel::local::channel", LevelFilter::Warn),
        ("h2", LevelFilter::Warn),
        ("hyper", LevelFilter::Info),
        ("tower_http::trace::on_request", LevelFilter::Warn),
        ("tower_http::trace::on_response", LevelFilter::Warn),
        ("tracing::span", LevelFilter::Warn),
    ];

    let config_log_level = config_log_level
        .unwrap_or("info".to_string())
        .to_lowercase();

    let level = if (debug || config_log_level == "debug") && env::var("RUST_LOG").is_err() {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    if level == LevelFilter::Info {
        filters.push(("ssh::", LevelFilter::Warn));
    }
    if !db_debug {
        // We don't want to see the SQL queries in the logs
        filters.push(("sea_orm::driver::sqlx_sqlite", LevelFilter::Error));
        filters.push(("sqlx::query", LevelFilter::Warn));
    }

    match tokio_console {
        true => {
            console_subscriber::init();
            println!("You're in tokio console mode, can't really log  :(");

            Ok(())
        }
        false => {
            let mut builder = Builder::from_default_env();
            builder.filter_level(level);
            builder.target(Target::Stdout);
            for (module, level) in filters {
                builder.filter_module(module, level);
            }

            #[cfg(not(test))]
            {
                builder.try_init()
            }

            #[cfg(test)]
            {
                if let Err(err) = builder.try_init() {
                    use tracing::debug;
                    debug!("Error init logging: {:?}", err);
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::setup_logging;

    #[test]
    fn test_setup_logging() {
        let test1 = setup_logging(None, false, true, false);
        dbg!(&test1);
        assert!(test1.is_ok());

        // it'll probably throw an error because we're trying to re-init the logger, but we're in test so it's OK.
        let test2 = setup_logging(None, true, true, false);
        dbg!(&test2);
        assert!(test2.is_ok());

        let test3 = setup_logging(None, true, false, false);
        dbg!(&test3);
        assert!(test3.is_ok());
        let testf = setup_logging(Some("debug".to_string()), false, false, false);
        dbg!(&testf);
        assert!(testf.is_ok());
    }
}
