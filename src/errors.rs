//! Generic error things

use axum::http::StatusCode;
use axum::response::IntoResponse;
#[cfg(not(tarpaulin_include))]
use axum::response::Response;
use croner::errors::CronError;
use kube::config::KubeconfigError;
use tokio::sync::oneshot;
use tracing::error;
use uuid::Uuid;

use crate::constants::{CSRF_TOKEN_MISMATCH, CSRF_TOKEN_NOT_FOUND};

#[derive(Debug, PartialEq)]
/// Various errors that Maremma will throw
pub enum Error {
    /// You didn't include the CSRF token in your form
    CsrfTokenMissing,
    /// You're not allowed to do this!
    Unauthorized,
    /// We couldn't find the config file
    ConfigFileNotFound(String),
    /// When the configuration is invalid
    Configuration(String),
    /// When the connection to the database failed
    ConnectionFailed,
    /// When the cron pattern is invalid
    CronParseError(String),
    /// CSRF token validation failed
    CsrfValidationFailed,
    /// When the date is in the future
    DateIsInTheFuture,
    /// Failed to deserialize a value
    Deserialization(String),
    /// When the DNS lookup failed
    DnsFailed,
    /// When we haven't made up an error otherwise
    Generic(String),
    /// When the host group is not found
    HostGroupNotFoundByName(String),
    /// When the host is not found
    HostNotFound(Uuid),
    /// When you've specified something wrong
    InvalidInput(String),
    /// When the IO operation failed
    IoError(String),
    /// K8s things
    KubeError(String),
    /// Something you asked for isn't implemented yet
    NotImplemented,
    /// Oneshot command failed
    OneShotFailed,
    /// When the OIDC token is invalid or some other error gets thrown
    Oidc(String),
    /// When something went wrong while invoking reqwest
    Reqwest(String),
    /// Something relating to the backend session store went wrong
    Session(String),
    /// When the service check is not found
    ServiceCheckNotFound(Uuid),
    /// When the service is not found
    ServiceConfigNotFound(String),
    /// When the service is not found
    ServiceNotFound(Uuid),
    /// When the service is not found
    ServiceNotFoundByName(String),
    /// When the SQL operation failed
    SqlError(sea_orm::error::DbErr),
    /// When the TLS operation failed
    TlsError(String),
    /// When the timeout is reached
    Timeout,

    /// When we fail to receive a message from the channel
    IPCRecvError(String),
    /// When we fail to send a message into the channel
    IPCSendError(String),
    /// You specified a CLI command but it wasn't found
    CommandNotFound(String),
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Deserialization(err.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::IoError(err.to_string())
    }
}

impl From<sea_orm::error::DbErr> for Error {
    fn from(err: sea_orm::error::DbErr) -> Self {
        Error::SqlError(err)
    }
}

impl From<CronError> for Error {
    fn from(value: CronError) -> Self {
        Error::CronParseError(value.to_string())
    }
}

#[cfg(not(tarpaulin_include))]
impl From<axum_oidc::error::Error> for Error {
    fn from(value: axum_oidc::error::Error) -> Self {
        Error::Oidc(value.to_string())
    }
}

#[cfg(not(tarpaulin_include))]
impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        Self::Reqwest(value.to_string())
    }
}

#[cfg(not(tarpaulin_include))]
impl From<rustls::Error> for Error {
    fn from(value: rustls::Error) -> Self {
        Self::TlsError(value.to_string())
    }
}

#[cfg(not(tarpaulin_include))]
impl From<tower_sessions::session::Error> for Error {
    fn from(value: tower_sessions::session::Error) -> Self {
        Self::Session(value.to_string())
    }
}

#[cfg(not(tarpaulin_include))]
impl From<tower_sessions::session_store::Error> for Error {
    fn from(value: tower_sessions::session_store::Error) -> Self {
        Self::Session(value.to_string())
    }
}

impl From<Error> for (StatusCode, String) {
    fn from(value: Error) -> Self {
        error!("{:?}", value);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Please see server logs".to_string(),
        )
    }
}

impl From<kube::Error> for Error {
    fn from(value: kube::Error) -> Self {
        Self::KubeError(value.to_string())
    }
}

impl From<KubeconfigError> for Error {
    fn from(value: KubeconfigError) -> Self {
        Self::KubeError(value.to_string())
    }
}

impl From<std::net::AddrParseError> for Error {
    fn from(value: std::net::AddrParseError) -> Self {
        Self::InvalidInput(format!("invalid IP address: {value}"))
    }
}

impl From<axum::http::header::InvalidHeaderValue> for Error {
    fn from(value: axum::http::header::InvalidHeaderValue) -> Self {
        Self::InvalidInput(value.to_string())
    }
}
#[cfg(not(tarpaulin_include))]
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        match self {
            Self::CsrfTokenMissing => (StatusCode::FORBIDDEN, CSRF_TOKEN_NOT_FOUND.to_string()),
            Self::CsrfValidationFailed => (StatusCode::FORBIDDEN, CSRF_TOKEN_MISMATCH.to_string()),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            _ => {
                error!("Response error occurred: {:?}", self);
                (StatusCode::INTERNAL_SERVER_ERROR, format!("{self:?}"))
            }
        }
        .into_response()
    }
}

impl From<oneshot::error::RecvError> for Error {
    fn from(value: oneshot::error::RecvError) -> Self {
        Self::IPCRecvError(value.to_string())
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for Error
where
    T: std::fmt::Debug,
{
    fn from(value: tokio::sync::mpsc::error::SendError<T>) -> Self {
        Self::IPCSendError(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use axum::response::IntoResponse;
    use reqwest::StatusCode;

    #[test]
    fn test_error_from_serde_json_error() {
        #[allow(clippy::unwrap_used)]
        let err = serde_json::from_str::<String>("{").unwrap_err();
        assert_eq!(
            crate::errors::Error::Deserialization(err.to_string()),
            crate::errors::Error::from(err)
        );
    }

    #[test]
    fn test_error_from_std_io_error() {
        let err = std::io::Error::other("test");
        assert_eq!(
            crate::errors::Error::IoError(err.to_string()),
            crate::errors::Error::from(err)
        );
    }

    #[test]
    fn test_error_from_sea_orm_error() {
        assert_eq!(
            crate::errors::Error::SqlError(sea_orm::error::DbErr::Json("test".to_string())),
            crate::errors::Error::from(sea_orm::error::DbErr::Json("test".to_string()))
        );
    }

    #[test]
    fn test_error_from_cronerror() {
        assert_eq!(
            crate::errors::Error::CronParseError(
                "CronPattern cannot be an empty string.".to_string()
            ),
            crate::errors::Error::from(croner::errors::CronError::EmptyPattern)
        );
    }

    #[test]
    fn error_into_response() {
        let err = crate::errors::Error::Generic("test".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let err = crate::errors::Error::Generic("test".to_string());
        let (status, _body) = err.into();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn error_from_kube_error() {
        let err = kube::Error::LinesCodecMaxLineLengthExceeded;
        assert_eq!(
            crate::errors::Error::KubeError(err.to_string()),
            crate::errors::Error::from(err)
        );
    }
    #[test]
    fn error_from_kubeconfig_error() {
        let err = kube::config::KubeconfigError::CurrentContextNotSet;
        assert_eq!(
            crate::errors::Error::KubeError(err.to_string()),
            crate::errors::Error::from(err)
        );
    }

    #[test]
    fn error_from_addrparseerror() {
        #[allow(clippy::unwrap_used)]
        let err = std::net::IpAddr::from_str("Invalid IP address").unwrap_err();
        assert_eq!(
            crate::errors::Error::InvalidInput(
                "invalid IP address: invalid IP address syntax".to_string()
            ),
            crate::errors::Error::from(err)
        );
    }
}
