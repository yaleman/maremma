// #![warn(missing_docs)]
#![deny(warnings)]
#![forbid(unsafe_code)]
#![deny(clippy::all)]
#![deny(clippy::correctness)]
#![deny(clippy::complexity)]
#![allow(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unreachable)]
#![deny(clippy::needless_pass_by_value)]
#![deny(clippy::await_holding_lock)]
#![deny(clippy::trivially_copy_pass_by_ref)]

#[cfg(not(test))]
use std::env;

use env_logger::{Builder, Target};
use log::LevelFilter;

pub mod check_loop;
pub mod cli;
pub mod config;
pub mod constants;
pub mod db;
pub mod errors;
pub mod host;
pub mod metrics;
pub mod prelude;
pub(crate) mod serde;
pub mod services;
#[cfg(test)]
pub(crate) mod tests;
pub mod web;

/// The default filename - `maremma.json`
pub static DEFAULT_CONFIG_FILE: &str = "maremma.json";
/// Used to give the "local" services a hostname
pub static LOCAL_SERVICE_HOST_NAME: &str = "Maremma Local Checks";

#[inline]
pub fn setup_logging(debug: bool) -> Result<(), log::SetLoggerError> {
    #[cfg(not(test))]
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    let mut builder = Builder::from_default_env();
    if debug {
        builder.filter_level(LevelFilter::Debug);
    }

    #[cfg(not(all(test, debug_assertions)))]
    builder.filter(Some("sqlx::query"), LevelFilter::Warn);
    builder.filter(Some("tracing::span"), LevelFilter::Warn);
    #[cfg(not(test))]
    builder.target(Target::Stdout);
    #[cfg(test)]
    builder.target(Target::Stderr);
    builder.try_init()
}
