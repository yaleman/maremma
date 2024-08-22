//! Maremma constant values

/// Default listener port
pub static WEB_SERVER_DEFAULT_PORT: u16 = 8888;

/// Default location for the static resources
pub static WEB_SERVER_DEFAULT_STATIC_PATH: &str = "./static";

/// Default number of history entries to show on the service check page
pub static DEFAULT_SERVICE_CHECK_HISTORY_LIMIT: u64 = 50;

/// Expiry time + x hours is when we clean up old sessions from the DB
pub(crate) static SESSION_EXPIRY_WINDOW_HOURS: i64 = 8;
