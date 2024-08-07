use croner::errors::CronError;
use uuid::Uuid;

#[derive(Debug, PartialEq)]
pub enum Error {
    DNSFailed,
    ConfigFileNotFound(String),
    ConnectionFailed,
    Generic(String),
    ConfigParse(String),
    IoError(String),
    ServiceNotFoundByName(String),
    ServiceNotFound(Uuid),
    HostGroupNotFoundByName(String),
    HostNotFound(Uuid),
    ServiceCheckNotFound(Uuid),
    ServiceConfigNotFound(String),
    SqlError(sea_orm::error::DbErr),
    CronParseError(String),
    InvalidInput(String),
    DateIsInTheFuture,
    Oidc(String),
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::ConfigParse(err.to_string())
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

impl From<axum_oidc::error::Error> for Error {
    fn from(value: axum_oidc::error::Error) -> Self {
        Error::Oidc(value.to_string())
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_error_from_serde_json_error() {
        let err = serde_json::from_str::<String>("{").unwrap_err();
        assert_eq!(
            crate::errors::Error::ConfigParse(err.to_string()),
            crate::errors::Error::from(err)
        );
    }

    #[test]
    fn test_error_from_std_io_error() {
        let err = std::io::Error::new(std::io::ErrorKind::Other, "test");
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
}
