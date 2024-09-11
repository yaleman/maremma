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
pub const DEFAULT_SERVICE_CHECK_HISTORY_LIMIT: u64 = 50;

/// Expiry time + x hours is when we clean up old sessions from the DB
pub(crate) const SESSION_EXPIRY_WINDOW_HOURS: i64 = 8;

/// How many minutes a check will be in "Checking" state before we consider it stuck
pub const STUCK_CHECK_MINUTES: i64 = 5;

/// Just so we don't typo things
pub(crate) const SESSION_CSRF_TOKEN: &str = "csrf_token";
