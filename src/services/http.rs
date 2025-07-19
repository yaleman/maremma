//! HTTP Checks

use std::fmt::Display;
use std::num::NonZeroU16;
use std::path::PathBuf;

use super::prelude::*;
use crate::prelude::*;
use reqwest::redirect::Policy;
use reqwest::{Response, StatusCode};
use schemars::JsonSchema;

#[derive(Debug, Deserialize, Serialize, Default, Copy, Clone, Eq, PartialEq)]
#[serde(rename_all = "UPPERCASE", from = "String", into = "String")]
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

impl From<HttpMethod> for String {
    fn from(value: HttpMethod) -> Self {
        value.to_string()
    }
}

/// Crimes against strings
impl Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            format!("{self:?}")
                .split(':')
                .next_back()
                .ok_or(std::fmt::Error)?
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

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
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

    /// Expected status code, defaults to 200
    pub http_status: Option<NonZeroU16>,

    /// Validate TLS, defaults to True
    #[serde(default = "default_true")]
    pub validate_tls: bool,

    /// Connection timeout, defaults to 10 seconds ([DEFAULT_TIMEOUT])
    pub connect_timeout: Option<u64>,

    /// Port to connect to, defaults to 443 (https)
    pub port: Option<NonZeroU16>,

    /// Ensure the body has a certain string
    pub contains_string: Option<String>,

    /// CA cert file to use
    pub ca_file: Option<PathBuf>,

    /// Actually use HTTP, not HTTPS...
    pub use_http: Option<bool>,

    /// Add random jitter in 0..n seconds to the check
    pub jitter: Option<u16>,
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

        // if we're looking for a string, we need to read the body and check for it
        if let Some(expected_string) = client_config.contains_string.as_ref() {
            body = response.text().await?;
            if !body.contains(expected_string) {
                debug!("Couldn't find {} in body", expected_string);
                return Ok((
                    format!("Expected string '{expected_string}' not found in body"),
                    ServiceStatus::Critical,
                ));
            } else {
                debug!("Found '{}' in body", expected_string);
            }
        } else {
            trace!("{}", body);
        }

        Ok(("OK".to_string(), ServiceStatus::Ok))
    }
}

#[tokio::test]
async fn test_overlay_host_config() {
    let _ = test_setup().await.expect("Failed to setup test");

    let service = HttpService {
        name: "test".to_string(),
        cron_schedule: std::str::FromStr::from_str("@hourly")
            .expect("Failed to parse @hourly"),
        http_method: HttpMethod::Get,
        http_uri: None,
        http_status: None,
        validate_tls: false,
        connect_timeout: None,
        port: None,
        use_http: None,
        contains_string: None,
        ca_file: None,
        jitter: None,
    };
    let mut value = Map::new();
    value.insert("port".to_string(), 12345.into());
    value.insert("http_uri".to_string(), "/asdfsafd".into());
    value.insert("cron_schedule".to_string(), "@daily".into());
    value.insert("ca_file".to_string(), "/dev/null".into());

    debug!("Overlay Value: {:?}", value);

    let res = service
        .overlay_host_config(&value)
        .expect("Failed to get partial");

    assert_ne!(service.http_uri, res.http_uri);
    assert_ne!(service.port, res.port);
    assert_ne!(
        service.cron_schedule.pattern.to_string(),
        res.cron_schedule.pattern.to_string()
    );
    // Both @daily and "0 0 * * *" are equivalent
    let cron_pattern = res.cron_schedule.pattern.to_string();
    assert!(cron_pattern == "@daily" || cron_pattern == "0 0 * * *", "Expected @daily or '0 0 * * *', got: {}", cron_pattern);
    assert_eq!(res.ca_file, Some(PathBuf::from("/dev/null")));
}

impl ConfigOverlay for HttpService {
    fn overlay_host_config(&self, value: &Map<String, Value>) -> Result<Box<Self>, Error> {
        let name = self.extract_string(value, "name", &self.name);

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

        Ok(Box::new(Self {
            name,
            cron_schedule: self.extract_cron(value, "cron_schedule", &self.cron_schedule)?,
            http_method: self.extract_value(value, "http_method", &self.http_method)?,
            http_uri: self.extract_value(value, "http_uri", &self.http_uri)?,
            http_status,
            validate_tls: self.extract_bool(value, "validate_tls", self.validate_tls),
            connect_timeout: self.extract_value(value, "connect_timeout", &self.connect_timeout)?,
            port: self.extract_value(value, "port", &self.port)?,
            contains_string: self.extract_value(value, "contains_string", &self.contains_string)?,
            ca_file: self.extract_value(value, "ca_file", &self.ca_file)?,
            use_http: self.extract_value(value, "use_http", &self.use_http)?,
            jitter: self.extract_value(value, "jitter", &self.jitter)?,
        }))
    }
}

#[async_trait]
impl ServiceTrait for HttpService {
    fn validate(&self) -> Result<(), Error> {
        if let Some(http_status) = self.http_status {
            if StatusCode::try_from(u16::from(http_status)).is_err() {
                return Err(Error::Configuration(format!(
                    "Invalid HTTP status code: {http_status}"
                )));
            }
        }
        Ok(())
    }

    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        let start_time = chrono::Utc::now();

        // get the client config
        debug!("Getting host config for host_id={}", host.id);

        let config = self.overlay_host_config(&self.get_host_config(&self.name, host)?)?;

        let scheme = if config.use_http.unwrap_or(false) {
            "http"
        } else {
            "https"
        };

        let url = format!(
            "{}://{}{}{}",
            scheme,
            host.hostname,
            config
                .port
                .map(|p| format!(":{p}"))
                .unwrap_or("".to_string()),
            config.http_uri.as_ref().unwrap_or(&"".to_string())
        );

        let mut client = reqwest::ClientBuilder::new()
            .user_agent(format!(
                "{}/{}",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION")
            ))
            .danger_accept_invalid_certs(!config.validate_tls)
            .danger_accept_invalid_hostnames(!config.validate_tls)
            // don't allow us to be redirected!
            .redirect(Policy::none());

        if let Some(ca_file) = config.ca_file.as_ref() {
            debug!("adding CA file");
            client = client.add_root_certificate(reqwest::Certificate::from_pem(
                &std::fs::read(ca_file).map_err(|e| {
                    Error::Generic(format!(
                        "Failed to read CA file {}: {}",
                        ca_file.display(),
                        e
                    ))
                })?,
            )?);
        }
        let client = client
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
            Err(err) => (format!("{err:?}"), ServiceStatus::Critical),
        };

        let time_elapsed = chrono::Utc::now() - start_time;

        Ok(CheckResult {
            timestamp: start_time,
            result_text,
            status,
            time_elapsed,
        })
    }

    fn as_json_pretty(&self, host: &entities::host::Model) -> Result<String, Error> {
        let config = self.overlay_host_config(&self.get_host_config(&self.name, host)?)?;
        Ok(serde_json::to_string_pretty(&config)?)
    }

    fn jitter_value(&self) -> u32 {
        self.jitter.unwrap_or(0) as u32
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    use crate::db::tests::test_setup;
    use crate::tests::testcontainers::TestContainer;
    use crate::tests::tls_utils::TestCertificateBuilder;
    use crate::web::urls::Urls;

    #[tokio::test]
    async fn test_httpservice() {
        let service = super::HttpService {
            name: "test".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_method: crate::services::http::HttpMethod::Get,
            validate_tls: true,
            connect_timeout: Some(5),
            port: None,
            http_uri: None,
            contains_string: None,
            http_status: None,
            ca_file: None,
            jitter: None,
            use_http: None,
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
        dbg!(&res);
        assert!(res.is_ok());
        assert_eq!(res.expect("failed to run").status, ServiceStatus::Ok);
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
    async fn test_site_contains_string() {
        let _ = test_setup().await.expect("Failed to setup test");

        let certs = TestCertificateBuilder::new()
            .with_name("localhost")
            .with_expiry((chrono::Utc::now() + chrono::TimeDelta::days(30)).timestamp())
            .with_issue_time((chrono::Utc::now() - chrono::TimeDelta::days(30)).timestamp())
            .build();

        let test_container = TestContainer::new(&certs, "test_site_contains_string").await;

        let service = super::HttpService {
            name: "test".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_method: crate::services::http::HttpMethod::Get,
            http_uri: Some(Urls::Index.to_string()),
            http_status: Some(super::default_expected_http_status()),
            validate_tls: true,
            connect_timeout: Some(5),
            port: Some(
                NonZeroU16::new(test_container.published_port).expect("Failed to parse port"),
            ),
            contains_string: Some("Welcome to nginx!".to_string()),
            ca_file: Some(PathBuf::from(certs.ca_file.as_ref())),
            jitter: None,
            use_http: None,
        };
        let mut host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "localhost".to_string(),
            hostname: "localhost".to_string(),
            check: crate::host::HostCheck::None,
            config: json!({}),
        };

        let res = service.run(&host).await;
        dbg!(&res);
        assert_eq!(service.name, "test".to_string());
        assert!(res.is_ok());
        assert_eq!(res.expect("failed to run").status, ServiceStatus::Ok);

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
        assert_eq!(res.expect("failed to run").status, ServiceStatus::Critical);
    }

    #[tokio::test]
    async fn test_github_com_status_code() {
        let _ = test_setup().await.expect("Failed to setup test");

        let service = super::HttpService {
            name: "test".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_status: Some(NonZeroU16::new(301).expect("failed to parse 301 as non-zero u16")),
            http_method: HttpMethod::Get,
            http_uri: None,
            validate_tls: true,
            connect_timeout: None,
            port: None,
            contains_string: None,
            ca_file: None,
            jitter: None,
            use_http: Some(true),
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
        println!("{res:?}");
        assert_eq!(res.expect("failed to run").status, ServiceStatus::Ok);

        // if we put this text on the page, we're just kicking ourselves in the shins
        host.config = json!({
            "test": {
                "http_status": 404,
                "connect_timeout" : 5,
                "port" : 443,
            }
        });

        dbg!(&host);

        let res = service.run(&host).await;

        dbg!(&res);
        assert_eq!(service.name, "test".to_string());
        assert!(res.is_ok());
        assert_eq!(res.expect("failed to run").status, ServiceStatus::Critical);
    }

    #[tokio::test]
    async fn test_skip_tls_verify() {
        let _ = test_setup().await.expect("Failed to setup test");

        let certs = TestCertificateBuilder::new()
            .with_name("asdfasdfdsf")
            .with_expiry((chrono::Utc::now() - chrono::TimeDelta::days(30)).timestamp())
            .with_issue_time((chrono::Utc::now() - chrono::TimeDelta::days(31)).timestamp())
            .build();

        let test_container = TestContainer::new(&certs, "test_skip_tls_verify").await;

        let service = super::HttpService {
            name: "localhost".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_method: crate::services::http::HttpMethod::Get,
            http_uri: None,
            http_status: Some(super::default_expected_http_status()),
            validate_tls: false,
            connect_timeout: Some(15),
            port: NonZeroU16::new(test_container.published_port),
            contains_string: None,
            ca_file: None,
            jitter: None,
            use_http: None,
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "localhost".to_string(),
            hostname: "localhost".to_string(),
            check: crate::host::HostCheck::None,
            config: json!({}),
        };

        let res = service.run(&host).await;
        assert_eq!(service.name, "localhost".to_string());
        dbg!(&res);
        assert!(res.is_ok());
        assert_eq!(res.expect("failed to run").status, ServiceStatus::Ok);

        drop(test_container);

        let certs = TestCertificateBuilder::new()
            .with_name("localhost")
            .with_expiry((chrono::Utc::now() - chrono::TimeDelta::days(30)).timestamp())
            .with_issue_time((chrono::Utc::now() - chrono::TimeDelta::days(31)).timestamp())
            .build();

        let test_container = TestContainer::new(&certs, "test_skip_tls_verify").await;

        let service = super::HttpService {
            name: "localhost".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_method: crate::services::http::HttpMethod::Get,
            http_uri: None,
            http_status: None,
            validate_tls: false,
            connect_timeout: Some(15),
            port: NonZeroU16::new(test_container.published_port),
            contains_string: None,
            ca_file: None,
            jitter: None,
            use_http: None,
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "localhost".to_string(),
            hostname: "localhost".to_string(),
            check: crate::host::HostCheck::None,
            config: json!({}),
        };

        let res = service.run(&host).await;
        assert_eq!(service.name, "localhost".to_string());
        dbg!(&res);
        assert!(res.is_ok());
        assert_eq!(res.expect("failed to run").status, ServiceStatus::Ok);
        drop(test_container);
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
            contains_string: None,
            ca_file: None,
            jitter: None,
            use_http: None,
        };

        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: "localhost".to_string(),
            check: crate::host::HostCheck::None,
            config: json!({}),
        };

        let res = service.run(&host).await;
        dbg!(&res);
        assert_eq!(service.name, "test".to_string());
        assert!(res.is_ok());
        assert_eq!(res.expect("failed to run").status, ServiceStatus::Critical);
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
            ca_file: None,
            jitter: None,
            use_http: None,
        };

        let client_config = Box::new(service.clone());

        assert!(service.expected_status_code(&client_config).is_err());
    }
}
