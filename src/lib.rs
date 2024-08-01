pub mod cli;
pub mod config;
pub mod errors;
pub mod host;
pub mod prelude;
pub(crate) mod serde;
pub mod services;

pub static DEFAULT_CONFIG_FILE: &str = "maremma.json";
/// Used to give the "local" services a hostname
pub static LOCAL_SERVICE_HOST_NAME: &str = "Maremma Local Checks";
