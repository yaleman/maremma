//! Messages for controlling the server backend
//!

pub enum WebServerControl {
    Stop,
    /// Stop the server after a certain amount of milliseconds (1000 = 1 second)
    StopAfter(u64),
    Reload,
    /// Reload the server after a certain amount of milliseconds (1000 = 1 second)
    ReloadAfter(u64),
}
