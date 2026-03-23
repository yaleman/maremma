//! Generic error things

use crate::prelude::*;
use askama::Template;
use askama_web::WebTemplate;
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
pub enum MaremmaError {
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
    /// When the host group membership is not found
    HostGroupMembershipNotFound(Uuid, Uuid),
    /// When the host group is not found
    HostGroupNotFound(Uuid),
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

impl From<&MaremmaError> for StatusCode {
    fn from(value: &MaremmaError) -> Self {
        match value {
            MaremmaError::CsrfTokenMissing | MaremmaError::CsrfValidationFailed => {
                StatusCode::FORBIDDEN
            }
            MaremmaError::Unauthorized => StatusCode::UNAUTHORIZED,
            MaremmaError::HostGroupMembershipNotFound(_, _)
            | MaremmaError::HostGroupNotFound(_)
            | MaremmaError::HostGroupNotFoundByName(_)
            | MaremmaError::HostNotFound(_)
            | MaremmaError::ServiceConfigNotFound(_)
            | MaremmaError::ServiceNotFound(_)
            | MaremmaError::ServiceNotFoundByName(_)
            | MaremmaError::ServiceCheckNotFound(_) => StatusCode::NOT_FOUND,
            MaremmaError::ConfigFileNotFound(_)
            | MaremmaError::Configuration(_)
            | MaremmaError::ConnectionFailed
            | MaremmaError::CronParseError(_)
            | MaremmaError::InvalidInput(_)
            | MaremmaError::DateIsInTheFuture
            | MaremmaError::Deserialization(_)
            | MaremmaError::Generic(_)
            | MaremmaError::OneShotFailed
            | MaremmaError::IoError(_)
            | MaremmaError::KubeError(_)
            | MaremmaError::NotImplemented
            | MaremmaError::Oidc(_)
            | MaremmaError::Reqwest(_)
            | MaremmaError::Session(_)
            | MaremmaError::DnsFailed
            | MaremmaError::SqlError(_)
            | MaremmaError::TlsError(_)
            | MaremmaError::Timeout
            | MaremmaError::IPCRecvError(_)
            | MaremmaError::IPCSendError(_)
            | MaremmaError::CommandNotFound(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<MaremmaError> for StatusCode {
    fn from(value: MaremmaError) -> Self {
        match value {
            MaremmaError::CsrfTokenMissing | MaremmaError::CsrfValidationFailed => {
                StatusCode::FORBIDDEN
            }
            MaremmaError::Unauthorized => StatusCode::UNAUTHORIZED,
            MaremmaError::HostGroupMembershipNotFound(_, _)
            | MaremmaError::HostGroupNotFound(_)
            | MaremmaError::HostGroupNotFoundByName(_)
            | MaremmaError::HostNotFound(_)
            | MaremmaError::ServiceConfigNotFound(_)
            | MaremmaError::ServiceNotFound(_)
            | MaremmaError::ServiceNotFoundByName(_)
            | MaremmaError::ServiceCheckNotFound(_) => StatusCode::NOT_FOUND,
            MaremmaError::ConfigFileNotFound(_)
            | MaremmaError::Configuration(_)
            | MaremmaError::ConnectionFailed
            | MaremmaError::CronParseError(_)
            | MaremmaError::InvalidInput(_)
            | MaremmaError::DateIsInTheFuture
            | MaremmaError::Deserialization(_)
            | MaremmaError::Generic(_)
            | MaremmaError::OneShotFailed
            | MaremmaError::IoError(_)
            | MaremmaError::KubeError(_)
            | MaremmaError::NotImplemented
            | MaremmaError::Oidc(_)
            | MaremmaError::Reqwest(_)
            | MaremmaError::Session(_)
            | MaremmaError::DnsFailed
            | MaremmaError::SqlError(_)
            | MaremmaError::TlsError(_)
            | MaremmaError::Timeout
            | MaremmaError::IPCRecvError(_)
            | MaremmaError::IPCSendError(_)
            | MaremmaError::CommandNotFound(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<serde_json::Error> for MaremmaError {
    fn from(err: serde_json::Error) -> Self {
        MaremmaError::Deserialization(err.to_string())
    }
}

impl From<std::io::Error> for MaremmaError {
    fn from(err: std::io::Error) -> Self {
        MaremmaError::IoError(err.to_string())
    }
}

impl From<sea_orm::error::DbErr> for MaremmaError {
    fn from(err: sea_orm::error::DbErr) -> Self {
        MaremmaError::SqlError(err)
    }
}

impl From<CronError> for MaremmaError {
    fn from(value: CronError) -> Self {
        MaremmaError::CronParseError(value.to_string())
    }
}

#[cfg(not(tarpaulin_include))]
impl From<axum_oidc::error::Error> for MaremmaError {
    fn from(value: axum_oidc::error::Error) -> Self {
        MaremmaError::Oidc(value.to_string())
    }
}

#[cfg(not(tarpaulin_include))]
impl From<reqwest::Error> for MaremmaError {
    fn from(value: reqwest::Error) -> Self {
        Self::Reqwest(value.to_string())
    }
}

#[cfg(not(tarpaulin_include))]
impl From<rustls::Error> for MaremmaError {
    fn from(value: rustls::Error) -> Self {
        Self::TlsError(value.to_string())
    }
}

#[cfg(not(tarpaulin_include))]
impl From<tower_sessions::session::Error> for MaremmaError {
    fn from(value: tower_sessions::session::Error) -> Self {
        Self::Session(value.to_string())
    }
}

#[cfg(not(tarpaulin_include))]
impl From<tower_sessions::session_store::Error> for MaremmaError {
    fn from(value: tower_sessions::session_store::Error) -> Self {
        Self::Session(value.to_string())
    }
}

impl From<MaremmaError> for (StatusCode, String) {
    fn from(value: MaremmaError) -> Self {
        error!("{:?}", value);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Please see server logs".to_string(),
        )
    }
}

impl From<kube::Error> for MaremmaError {
    fn from(value: kube::Error) -> Self {
        Self::KubeError(value.to_string())
    }
}

impl From<KubeconfigError> for MaremmaError {
    fn from(value: KubeconfigError) -> Self {
        Self::KubeError(value.to_string())
    }
}

impl From<std::net::AddrParseError> for MaremmaError {
    fn from(value: std::net::AddrParseError) -> Self {
        Self::InvalidInput(format!("invalid IP address: {value}"))
    }
}

impl From<axum::http::header::InvalidHeaderValue> for MaremmaError {
    fn from(value: axum::http::header::InvalidHeaderValue) -> Self {
        Self::InvalidInput(value.to_string())
    }
}

#[derive(Template, WebTemplate)]
#[template(path = "error.html")]
struct ErrorPage {
    title: String,
    message: String,
    username: Option<String>,
}

impl ErrorPage {
    fn new(title: &impl ToString, message: String) -> Self {
        Self {
            title: title.to_string(),
            message,
            username: None,
        }
    }

    fn as_error(&self, error: MaremmaError) -> Response {
        let page_content = self.render().unwrap_or_else(|err| {
            error!("Failed to render error page: {:?}", err);
            "".to_string()
        });
        let mut response = Response::new(page_content.into());
        *response.status_mut() = error.into();
        response
    }
}

#[cfg(not(tarpaulin_include))]
impl IntoResponse for MaremmaError {
    fn into_response(self) -> Response {
        match &self {
            Self::CsrfTokenMissing => {
                (StatusCode::FORBIDDEN, CSRF_TOKEN_NOT_FOUND.to_string()).into_response()
            }
            Self::CsrfValidationFailed => {
                (StatusCode::FORBIDDEN, CSRF_TOKEN_MISMATCH.to_string()).into_response()
            }
            Self::Unauthorized => {
                (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()).into_response()
            }
            Self::ServiceCheckNotFound(check_id) => ErrorPage {
                title: "Service Check Not Found".to_string(),
                message: format!("No service check found with ID {check_id}"),
                username: None,
            }
            .as_error(self),
            Self::HostNotFound(host_id) => ErrorPage::new(
                &"Host Not Found",
                format!("No host found with ID {host_id}"),
            )
            .as_error(self),
            Self::HostGroupNotFoundByName(name) => ErrorPage::new(
                &"Host Group Not Found",
                format!("No host group found with name {name}"),
            )
            .as_error(self),
            Self::HostGroupNotFound(id) => ErrorPage::new(
                &"Host Group Not Found",
                format!("No host group found with ID {id}"),
            )
            .as_error(self),
            Self::ServiceNotFound(service_id) => ErrorPage::new(
                &"Service Not Found",
                format!("No service found with ID {service_id}"),
            )
            .as_error(self),
            Self::ServiceNotFoundByName(name) => ErrorPage::new(
                &"Service Not Found",
                format!("No service found with name {name}"),
            )
            .as_error(self),

            Self::ConfigFileNotFound(_)
            | Self::Configuration(_)
            | Self::ConnectionFailed
            | Self::CronParseError(_)
            | Self::DateIsInTheFuture
            | Self::Deserialization(_)
            | Self::DnsFailed
            | Self::Generic(_)
            | Self::HostGroupMembershipNotFound(_, _)
            | Self::InvalidInput(_)
            | Self::IoError(_)
            | Self::KubeError(_)
            | Self::NotImplemented
            | Self::OneShotFailed
            | Self::Oidc(_)
            | Self::Reqwest(_)
            | Self::Session(_)
            | Self::ServiceConfigNotFound(_)
            | Self::SqlError(_)
            | Self::TlsError(_)
            | Self::Timeout
            | Self::IPCRecvError(_)
            | Self::IPCSendError(_)
            | Self::CommandNotFound(_) => (
                StatusCode::from(&self),
                "Please see server logs".to_string(),
            )
                .into_response(),
        }
    }
}

impl From<oneshot::error::RecvError> for MaremmaError {
    fn from(value: oneshot::error::RecvError) -> Self {
        Self::IPCRecvError(value.to_string())
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for MaremmaError
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
            crate::errors::MaremmaError::Deserialization(err.to_string()),
            crate::errors::MaremmaError::from(err)
        );
    }

    #[test]
    fn test_error_from_std_io_error() {
        let err = std::io::Error::other("test");
        assert_eq!(
            crate::errors::MaremmaError::IoError(err.to_string()),
            crate::errors::MaremmaError::from(err)
        );
    }

    #[test]
    fn test_error_from_sea_orm_error() {
        assert_eq!(
            crate::errors::MaremmaError::SqlError(sea_orm::error::DbErr::Json("test".to_string())),
            crate::errors::MaremmaError::from(sea_orm::error::DbErr::Json("test".to_string()))
        );
    }

    #[test]
    fn test_error_from_cronerror() {
        assert_eq!(
            crate::errors::MaremmaError::CronParseError(
                "CronPattern cannot be an empty string.".to_string()
            ),
            crate::errors::MaremmaError::from(croner::errors::CronError::EmptyPattern)
        );
    }

    #[test]
    fn error_into_response() {
        let err = crate::errors::MaremmaError::Generic("test".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let err = crate::errors::MaremmaError::Generic("test".to_string());
        let (status, _body) = err.into();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn error_from_kube_error() {
        let err = kube::Error::LinesCodecMaxLineLengthExceeded;
        assert_eq!(
            crate::errors::MaremmaError::KubeError(err.to_string()),
            crate::errors::MaremmaError::from(err)
        );
    }
    #[test]
    fn error_from_kubeconfig_error() {
        let err = kube::config::KubeconfigError::CurrentContextNotSet;
        assert_eq!(
            crate::errors::MaremmaError::KubeError(err.to_string()),
            crate::errors::MaremmaError::from(err)
        );
    }

    #[test]
    fn error_from_addrparseerror() {
        #[allow(clippy::unwrap_used)]
        let err = std::net::IpAddr::from_str("Invalid IP address").unwrap_err();
        assert_eq!(
            crate::errors::MaremmaError::InvalidInput(
                "invalid IP address: invalid IP address syntax".to_string()
            ),
            crate::errors::MaremmaError::from(err)
        );
    }
}
