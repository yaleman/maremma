//! HTTP Checks

use std::fmt::Display;
use std::num::NonZeroU16;

use super::prelude::*;
use crate::prelude::*;
use reqwest::Response;
use schemars::JsonSchema;

#[derive(Debug, Deserialize, Default, Copy, Clone, Eq, PartialEq)]
#[serde(rename_all = "UPPERCASE", from = "String")]
/// HTTP Methods
#[allow(missing_docs)]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Options,
}

impl From<String> for HttpMethod {
    fn from(value: String) -> Self {
        match value.to_lowercase().as_str() {
            "get" => Self::Get,
            "post" => Self::Post,
            "put" => Self::Put,
            "delete" => Self::Delete,
            "patch" => Self::Patch,
            "options" => Self::Options,
            _ => Self::Get,
        }
    }
}

impl From<HttpMethod> for reqwest::Method {
    fn from(value: HttpMethod) -> Self {
        match value {
            HttpMethod::Get => Self::GET,
            HttpMethod::Post => Self::POST,
            HttpMethod::Put => Self::PUT,
            HttpMethod::Delete => Self::DELETE,
            HttpMethod::Patch => Self::PATCH,
            HttpMethod::Options => Self::OPTIONS,
        }
    }
}

/// Crimes against strings
impl Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            format!("{:?}", self)
                .split(':')
                .last()
                .ok_or_else(|| std::fmt::Error)?
                .to_ascii_uppercase()
                .as_str(),
        )
    }
}

fn default_true() -> bool {
    true
}

/// Default timeout for HTTP checks
pub const DEFAULT_TIMEOUT: u64 = 10;
/// Default expected status code for HTTP checks
fn default_expected_http_status() -> NonZeroU16 {
    #[allow(clippy::expect_used)]
    NonZeroU16::new(200).expect("Failed to parse 200 as a non-zero u16")
}

#[derive(Debug, Deserialize, JsonSchema, Clone)]
/// An HTTP(s) service check
pub struct HttpService {
    /// Name of the check
    pub name: String,

    #[serde(with = "crate::serde::cron")]
    #[schemars(with = "String")]
    /// Cron schedule for the service
    pub cron_schedule: Cron,

    /// Defaults to GET
    #[serde(default)]
    #[schemars(with = "String")]
    pub http_method: HttpMethod,

    /// Defaults to nothing (ie, no additional path)
    pub http_uri: Option<String>,

    /// Expected status code, defaults to 200 ([DEFAULT_HTTP_STATUS])
    pub http_status: Option<NonZeroU16>,

    /// Validate TLS, defaults to True
    #[serde(default = "default_true")]
    pub validate_tls: bool,

    /// Connection timeout, defaults to 10 seconds ([DEFAULT_TIMEOUT])
    pub connect_timeout: Option<u64>,

    /// Port to connect to, defaults to 443 (https)
    pub port: Option<u16>,

    /// Ensure the body has a certain string
    pub contains_string: Option<String>,
}

impl HttpService {
    /// Get the expected status code for the service and throw an error if it's bad
    fn expected_status_code(&self, client_config: &Self) -> Result<reqwest::StatusCode, Error> {
        reqwest::StatusCode::from_u16(
            client_config
                .http_status
                .unwrap_or(default_expected_http_status())
                .into(),
        )
        .map_err(|_| {
            Error::Generic(format!(
                "Invalid status code {} in service check",
                client_config
                    .http_status
                    .unwrap_or(default_expected_http_status())
            ))
        })
    }
    async fn validate_response(
        &self,
        response: Response,
        client_config: Box<HttpService>,
    ) -> Result<(String, ServiceStatus), Error> {
        let expected_status_code = self.expected_status_code(&client_config)?;

        if response.status() != expected_status_code {
            return Ok((
                format!(
                    "Expected status code {}, got {}",
                    expected_status_code,
                    response.status()
                ),
                ServiceStatus::Critical,
            ));
        };

        let mut body: String = String::new();

        dbg!(&client_config);

        if let Some(expected_string) = client_config.contains_string.as_ref() {
            body = response.text().await?;
            if !body.contains(expected_string) {
                return Ok((
                    format!("Expected string '{}' not found in body", expected_string),
                    ServiceStatus::Critical,
                ));
            } else {
                debug!("Found {} in body", expected_string);
            }
        } else {
            trace!("{}", body);
        }
        Ok(("OK".to_string(), ServiceStatus::Ok))
    }
}

#[test]
fn test_from_partial_value() {
    let service = HttpService {
        name: "test".to_string(),
        cron_schedule: Cron::new("@hourly")
            .parse()
            .expect("Failed to parse @hourly"),
        http_method: HttpMethod::Get,
        http_uri: None,
        http_status: None,
        validate_tls: false,
        connect_timeout: None,
        port: None,
        contains_string: None,
    };
    let mut value = Map::new();
    value.insert("port".to_string(), 12345.into());
    value.insert("http_uri".to_string(), "/asdfsafd".into());
    value.insert("cron_schedule".to_string(), "@daily".into());

    let res = service
        .overlay_host_config(&value)
        .expect("Failed to get partial");

    assert_ne!(service.http_uri, res.http_uri);
    assert_ne!(service.port, res.port);
    assert_ne!(
        service.cron_schedule.pattern.to_string(),
        res.cron_schedule.pattern.to_string()
    );
}

impl ConfigOverlay for HttpService {
    fn overlay_host_config(&self, value: &Map<String, Value>) -> Result<Box<Self>, Error> {
        let name = match value.get("name") {
            Some(val) => val.as_str().map(String::from).unwrap_or(self.name.clone()),
            None => self.name.clone(),
        };
        let cron_schedule = if value.contains_key("cron_schedule") {
            value
                .get("cron_schedule")
                .ok_or_else(|| Error::Generic("Failed to get cron_schedule".to_string()))?
                .as_str()
                .ok_or_else(|| Error::Generic("Failed to get cron_schedule".to_string()))?
                .parse()
                .map_err(|_| Error::Generic("Failed to parse cron_schedule".to_string()))?
        } else {
            self.cron_schedule.clone()
        };

        let http_method: HttpMethod = if value.contains_key("http_method") {
            value
                .get("http_method")
                .map(|val| serde_json::from_value(val.clone()))
                .transpose()?
                .unwrap_or(self.http_method)
        } else {
            self.http_method
        };

        let http_uri = match value.get("http_uri") {
            Some(val) => val.as_str().map(String::from),
            None => self.http_uri.clone(),
        };

        let http_status: Option<NonZeroU16> = match value.get("http_status") {
            Some(val) => match val.as_u64() {
                Some(val) => NonZeroU16::new(val as u16),
                None => match val.as_str() {
                    Some(val) => val.parse().ok(),
                    None => {
                        return Err(Error::Configuration(
                            "Couldn't parse http_status as valid number".to_string(),
                        ))
                    }
                },
            },
            None => self.http_status,
        };

        let validate_tls = match value.get("validate_tls") {
            Some(val) => val.as_bool().unwrap_or(self.validate_tls),
            None => self.validate_tls,
        };

        let connect_timeout = match value.get("connect_timeout") {
            Some(val) => val.as_u64(),
            None => self.connect_timeout,
        };

        let port = match value.get("port") {
            Some(val) => match val.as_u64() {
                Some(val) => Some(val as u16),
                None => match val.as_str() {
                    Some(val) => val.parse().ok(),
                    None => {
                        return Err(Error::Configuration(format!(
                            "Couldn't parse port from {} config ",
                            name
                        )));
                    }
                },
            },
            None => self.port,
        };

        let contains_string = match value.get("contains_string") {
            Some(val) => {
                debug!("Found contains_string in host config: {:?}", val);
                match val.as_str() {
                    Some(val) => Some(val.to_string()),
                    None => {
                        return Err(Error::Configuration(format!(
                            "Couldn't parse contains_string from {} config ",
                            name
                        )));
                    }
                }
            }
            None => self.contains_string.clone(),
        };

        Ok(Box::new(Self {
            name,
            cron_schedule,
            http_method,
            http_uri,
            http_status,
            validate_tls,
            connect_timeout,
            port,
            contains_string,
        }))
    }
}

#[async_trait]
impl ServiceTrait for HttpService {
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        let start_time = chrono::Utc::now();

        // get the client config
        let config = self.overlay_host_config(&self.get_host_config(&self.name, host)?)?;

        let url = format!(
            "https://{}{}/{}",
            host.hostname,
            config
                .port
                .map(|p| format!(":{}", p))
                .unwrap_or("".to_string()),
            config.http_uri.as_ref().unwrap_or(&"".to_string())
        );

        let client = reqwest::ClientBuilder::new()
            .user_agent(format!(
                "{}/{}",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION")
            ))
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .connect_timeout(std::time::Duration::from_secs(
                config.connect_timeout.unwrap_or(DEFAULT_TIMEOUT),
            ))
            .build()?;

        let (result_text, status) = match client
            .request(config.as_ref().http_method.into(), url)
            .send()
            .await
        {
            Ok(val) => self.validate_response(val, config).await?,
            Err(err) => (err.to_string(), ServiceStatus::Critical),
        };

        let time_elapsed = chrono::Utc::now() - start_time;

        Ok(CheckResult {
            timestamp: start_time,
            result_text,
            status,
            time_elapsed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::db::tests::test_setup;

    #[tokio::test]
    async fn test_httpservice() {
        let service = super::HttpService {
            name: "test".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_method: crate::services::http::HttpMethod::Post,
            validate_tls: true,
            connect_timeout: Some(5),
            port: None,
            http_uri: None,
            contains_string: None,
            http_status: None,
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: "example.com".to_string(),
            check: crate::host::HostCheck::None,
            config: json!({}),
        };

        let res = service.run(&host).await;
        assert_eq!(service.name, "test".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap().status, ServiceStatus::Ok);
        assert!(Service::try_from(&json! {
            {
                "name": "test",
                "run_in_shell": false,
                "service_type": "http",
            }
        })
        .is_err());
    }

    #[tokio::test]
    async fn test_example_com_contains_string() {
        let _ = test_setup().await.expect("Failed to setup test");

        let service = super::HttpService {
            name: "test".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_method: crate::services::http::HttpMethod::Get,
            http_uri: Some("/yaleman/maremma".to_string()),
            http_status: Some(super::default_expected_http_status()),
            validate_tls: true,
            connect_timeout: Some(5),
            port: None,
            contains_string: Some("Maremma".to_string()),
        };
        let mut host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: "github.com".to_string(),
            check: crate::host::HostCheck::None,
            config: json!({}),
        };

        let res = service.run(&host).await;
        assert_eq!(service.name, "test".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap().status, ServiceStatus::Ok);

        // if we put this text on the page, we're just kicking ourselves in the shins
        host.config = json!({
            "test": {
                "contains_string": "Purple Monkey Dishwasher"
            }
        });

        dbg!(&host);

        let res = service.run(&host).await;

        dbg!(&res);
        assert_eq!(service.name, "test".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap().status, ServiceStatus::Critical);
    }

    #[tokio::test]
    async fn test_github_com_status_code() {
        let _ = test_setup().await.expect("Failed to setup test");

        let service = super::HttpService {
            name: "test".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_status: Some(super::default_expected_http_status()),
            http_method: HttpMethod::Get,
            http_uri: None,
            validate_tls: true,
            connect_timeout: None,
            port: None,
            contains_string: None,
        };
        let mut host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: "github.com".to_string(),
            check: crate::host::HostCheck::None,
            config: json!({}),
        };

        let res = service.run(&host).await;
        assert_eq!(service.name, "test".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap().status, ServiceStatus::Ok);

        // if we put this text on the page, we're just kicking ourselves in the shins
        host.config = json!({
            "test": {
                "http_status": 404,
                "connect_timeout" : 5,
                "port" : "443",
            }
        });

        dbg!(&host);

        let res = service.run(&host).await;

        dbg!(&res);
        assert_eq!(service.name, "test".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap().status, ServiceStatus::Critical);
    }

    #[tokio::test]
    #[cfg(feature = "test_badssl")]
    async fn test_skip_tls_verify() {
        let service = super::HttpService {
            name: "test".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_method: crate::services::http::HttpMethod::Get,
            http_uri: None,
            http_status: Some(super::DEFAULT_EXPECTED_HTTP_STATUS),
            validate_tls: false,
            connect_timeout: Some(15),
            port: None,
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: "untrusted-root.badssl.com".to_string(),
            check: crate::host::HostCheck::None,
        };

        let res = service.run(&host).await;
        assert_eq!(service.name, "test".to_string());
        dbg!(&res);
        assert_eq!(res.is_ok(), true);
        assert_eq!(res.unwrap().status, ServiceStatus::Ok);

        let service = super::HttpService {
            name: "test".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_method: crate::services::http::HttpMethod::Get,
            http_uri: None,
            http_status: None,
            validate_tls: false,
            connect_timeout: Some(15),
            port: None,
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: "expired.badssl.com".to_string(),
            check: crate::host::HostCheck::None,
        };

        let res = service.run(&host).await;
        assert_eq!(service.name, "test".to_string());
        dbg!(&res);
        assert_eq!(res.is_ok(), true);
        assert_eq!(res.unwrap().status, ServiceStatus::Ok);

        // now we make sure it fails when we do validate

        let service = super::HttpService {
            name: "test".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_method: crate::services::http::HttpMethod::Get,
            http_uri: None,
            http_status: None,
            validate_tls: true,
            connect_timeout: Some(5),
            port: None,
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: "untrusted-root.badssl.com".to_string(),
            check: crate::host::HostCheck::None,
        };

        let res = service.run(&host).await;
        assert_eq!(service.name, "test".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap().status, ServiceStatus::Critical);
    }

    #[test]
    fn test_http_method_display() {
        assert_eq!(format!("{}", HttpMethod::Get), "GET");
        assert_eq!(format!("{}", HttpMethod::Post), "POST");
        assert_eq!(format!("{}", HttpMethod::Put), "PUT");
        assert_eq!(format!("{}", HttpMethod::Delete), "DELETE");
        assert_eq!(format!("{}", HttpMethod::Patch), "PATCH");
        assert_eq!(format!("{}", HttpMethod::Options), "OPTIONS");
    }

    #[test]
    fn test_http_method_from() {
        assert_eq!(reqwest::Method::GET, reqwest::Method::from(HttpMethod::Get));
        assert_eq!(
            reqwest::Method::POST,
            reqwest::Method::from(HttpMethod::Post)
        );
        assert_eq!(reqwest::Method::PUT, reqwest::Method::from(HttpMethod::Put));
        assert_eq!(
            reqwest::Method::DELETE,
            reqwest::Method::from(HttpMethod::Delete)
        );
        assert_eq!(
            reqwest::Method::PATCH,
            reqwest::Method::from(HttpMethod::Patch)
        );
        assert_eq!(
            reqwest::Method::OPTIONS,
            reqwest::Method::from(HttpMethod::Options)
        );
    }

    #[test]
    fn test_from_str_http_method() {
        assert_eq!(HttpMethod::Get, "get".to_string().into());
        assert_eq!(HttpMethod::Post, "post".to_string().into());
        assert_eq!(HttpMethod::Put, "put".to_string().into());
        assert_eq!(HttpMethod::Delete, "delete".to_string().into());
        assert_eq!(HttpMethod::Patch, "patch".to_string().into());
        assert_eq!(HttpMethod::Options, "options".to_string().into());
        assert_eq!(HttpMethod::Get, "nonsense".to_string().into());
    }

    #[test]
    fn test_default_expected_http_status() {
        assert_eq!(
            NonZeroU16::new(200).expect("Failed to parse 200 as a non-zero u16"),
            default_expected_http_status()
        );
    }

    #[test]
    fn test_parsing_invalid_http_status() {
        let service = serde_json::json!({
            "name": "test",
            "cron_schedule": "@hourly",
            "http_method": "get",
            "http_status": 0,
            "validate_tls": true,
            "connect_timeout": 5,
        });

        let service: Result<HttpService, serde_json::Error> = serde_json::from_value(service);

        dbg!(&service);
        assert!(service.is_err());
    }

    #[tokio::test]
    async fn test_expected_status_code() {
        let _ = test_setup().await.expect("Failed to setup test");

        let service = HttpService {
            name: "test".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_method: HttpMethod::Get,
            http_uri: None,
            http_status: NonZeroU16::new(13456),
            validate_tls: true,
            connect_timeout: Some(5),
            port: None,
            contains_string: None,
        };

        let client_config = Box::new(service.clone());

        assert!(service.expected_status_code(&client_config).is_err());
    }
}
