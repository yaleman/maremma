//! Maremma constant values

/// Default listener port
pub const WEB_SERVER_DEFAULT_PORT: u16 = 8888;

/// Default location for the static resources
pub const WEB_SERVER_DEFAULT_STATIC_PATH: &str = "./static";

/// Default number of history entries to show on the service check page
pub const DEFAULT_SERVICE_CHECK_HISTORY_LIMIT: u64 = 50;

/// Expiry time + x hours is when we clean up old sessions from the DB
pub(crate) const SESSION_EXPIRY_WINDOW_HOURS: i64 = 8;

/// How many minutes a check will be in "Checking" state before we consider it stuck
pub const STUCK_CHECK_MINUTES: i64 = 5;
