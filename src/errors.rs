#[derive(Debug, PartialEq)]
pub enum Error {
    DNSFailed,
    ConfigFileNotFound(String),
    ConnectionFailed,
    Generic(String),
    ConfigParse(String),
    IoError(String),
    ServiceNotFound,
    HostNotFound(String),
    ServiceCheckNotFound(String),
    ServiceConfigNotFound(String),
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
