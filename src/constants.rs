//! Maremma constant values

use std::num::NonZeroU16;

/// Default listener port
pub const WEB_SERVER_DEFAULT_PORT: u16 = 8888;

pub(crate) fn web_server_default_port() -> NonZeroU16 {
    #[allow(clippy::expect_used)]
    NonZeroU16::new(WEB_SERVER_DEFAULT_PORT).expect("Failed to parse WEB_SERVER_DEFAULT_PORT")
}

/// Default location for the static resources
pub const WEB_SERVER_DEFAULT_STATIC_PATH: &str = "./static";

/// Default number of history entries to show on the service check page
pub const DEFAULT_SERVICE_CHECK_HISTORY_VIEW_ENTRIES: u64 = 50;

/// Expiry time + x hours is when we clean up old sessions from the DB
pub(crate) const SESSION_EXPIRY_WINDOW_HOURS: i64 = 8;

/// How many minutes a check will be in "Checking" state before we consider it stuck
pub const STUCK_CHECK_MINUTES: i64 = 5;

/// Just so we don't typo things
pub(crate) const SESSION_CSRF_TOKEN: &str = "csrf_token";

/// Default number of history entries to keep in the database
pub const DEFAULT_SERVICE_CHECK_HISTORY_STORAGE: u64 = 25000;

/// When we can't find the CSRF token in the session
pub(crate) static CSRF_TOKEN_NOT_FOUND: &str = "CSRF Token wasn't found!";
/// When the CSRF token in the session doesn't match the one in the form
pub(crate) static CSRF_TOKEN_MISMATCH: &str = "CSRF token mismatch";
