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
    HostNotFound(Uuid),
    ServiceCheckNotFound(String),
    ServiceConfigNotFound(String),
    SqlError(sea_orm::error::DbErr),
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
}
