//! # Maremma
//!
//! Guarding your herd 🐐🐐 🐕
//!

#![warn(missing_docs)]
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

pub mod check_loop;
pub mod cli;
pub mod config;
pub mod constants;
pub mod db;
pub mod errors;
pub mod host;
pub mod log;
pub mod metrics;
pub mod prelude;
pub(crate) mod serde;
pub mod services;
pub mod shepherd;
#[cfg(test)]
pub(crate) mod tests;
pub mod web;

/// The default filename - `maremma.json`
pub const DEFAULT_CONFIG_FILE: &str = "maremma.json";
/// Used to give the "local" services a hostname
pub const LOCAL_SERVICE_HOST_NAME: &str = "Maremma Local Checks";
