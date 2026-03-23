//! Messages for controlling the server backend
//!

/// Control messages for the web server
pub enum WebServerControl {
    /// Stop the server immediately
    Stop,
    /// Stop the server after a certain amount of milliseconds (1000 = 1 second)
    StopAfter(u64),
    /// Reload the server immediately
    Reload,
    /// Reload the server after a certain amount of milliseconds (1000 = 1 second)
    ReloadAfter(u64),
}
