#![deny(warnings)]
#![forbid(unsafe_code)]
#![deny(clippy::all)]
// #![deny(clippy::unimplemented)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unreachable)]
#![deny(clippy::await_holding_lock)]
#![deny(clippy::needless_pass_by_value)]
#![deny(clippy::trivially_copy_pass_by_ref)]

#[cfg(not(test))]
use std::env;

use env_logger::{Builder, Target};

pub mod check_loop;
pub mod cli;
pub mod config;
pub mod constants;
pub mod db;
pub mod errors;
pub mod host;
pub mod prelude;
pub(crate) mod serde;
pub mod services;
pub mod web;

pub static DEFAULT_CONFIG_FILE: &str = "maremma.json";
/// Used to give the "local" services a hostname
pub static LOCAL_SERVICE_HOST_NAME: &str = "Maremma Local Checks";

pub fn setup_logging(debug: bool) -> Result<(), log::SetLoggerError> {
    #[cfg(not(test))]
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    let mut builder = Builder::from_default_env();
    if debug {
        builder.filter_level(tracing::log::LevelFilter::Debug);
    }

    #[cfg(not(all(test, debug_assertions)))]
    builder.filter(Some("sqlx::query"), tracing::log::LevelFilter::Warn);
    builder.filter(Some("tracing::span"), tracing::log::LevelFilter::Warn);
    #[cfg(not(test))]
    builder.target(Target::Stdout);
    #[cfg(test)]
    builder.target(Target::Stderr);
    builder.try_init()
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_default_config_file() {
        assert_eq!(super::DEFAULT_CONFIG_FILE, "maremma.json");
    }
}
