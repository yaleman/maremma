//! HTTP Checks

use std::fmt::Display;

use reqwest::Response;
use schemars::JsonSchema;
use serde_json::Map;

use crate::prelude::*;

#[derive(Debug, Deserialize, Default, Copy, Clone)]
#[serde(rename_all = "UPPERCASE")]
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
#[test]
fn test_http_method_display() {
    assert_eq!(format!("{}", HttpMethod::Get), "GET");
    assert_eq!(format!("{}", HttpMethod::Post), "POST");
    assert_eq!(format!("{}", HttpMethod::Put), "PUT");
    assert_eq!(format!("{}", HttpMethod::Delete), "DELETE");
    assert_eq!(format!("{}", HttpMethod::Patch), "PATCH");
    assert_eq!(format!("{}", HttpMethod::Options), "OPTIONS");
}

fn default_true() -> bool {
    true
}

/// Default timeout for HTTP checks
pub const DEFAULT_TIMEOUT: u64 = 10;
/// Default expected status code for HTTP checks
pub const DEFAULT_EXPECTED_HTTP_STATUS: u16 = 200;

#[derive(Debug, Deserialize, JsonSchema)]
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
    pub http_status: Option<u16>,

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
    fn new_from_partial_value(&self, value: &Map<String, Value>) -> Result<Self, Error> {
        let mut value = value.clone();
        if !value.contains_key("name") {
            value.insert("name".to_string(), Value::String(self.name.clone()));
        }
        if !value.contains_key("cron_schedule") {
            value.insert(
                "cron_schedule".to_string(),
                Value::String(self.cron_schedule.pattern.to_string()),
            );
        }

        let res = serde_json::from_value(json!(value))?;
        Ok(res)
    }

    async fn validate_response(
        &self,
        response: Response,
        client_config: Option<HttpService>,
    ) -> Result<(String, ServiceStatus), Error> {
        let expected_status_code = reqwest::StatusCode::from_u16(
            client_config
                .as_ref()
                .map_or(self.http_status, |c| c.http_status)
                .unwrap_or(DEFAULT_EXPECTED_HTTP_STATUS),
        )
        .map_err(|_| {
            Error::Generic(format!(
                "Invalid status code {} in service check",
                self.http_status.unwrap_or(DEFAULT_EXPECTED_HTTP_STATUS)
            ))
        })?;

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

        if let Some(expected_string) = client_config
            .as_ref()
            .map_or(self.contains_string.as_ref(), |c| {
                c.contains_string.as_ref()
            })
        {
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
    value.insert("http_uri".to_string(), "/asdfsafd".into());

    let res = service
        .new_from_partial_value(&value)
        .expect("Failed to get partial");

    assert_ne!(service.http_uri, res.http_uri);
}

#[async_trait]
impl ServiceTrait for HttpService {
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        let start_time = chrono::Utc::now();

        // get the client config
        let client_config: Option<HttpService> = match host.config.get(self.name.clone()) {
            Some(val) => {
                debug!("found extra client config");
                if let Some(val) = val.as_object() {
                    Some(self.new_from_partial_value(val)?)
                } else {
                    None
                }
            }
            None => None,
        };

        // if we have a client config, we merge it with the default
        let http_method = client_config
            .as_ref()
            .map_or(self.http_method, |c| c.http_method);

        let url = format!(
            "https://{}{}/{}",
            host.hostname,
            client_config
                .as_ref()
                .map_or(self.port, |c| c.port)
                .map_or("".to_string(), |p| format!(":{}", p)),
            client_config
                .as_ref()
                .map_or(self.http_uri.clone(), |c| c.http_uri.clone())
                .unwrap_or("".to_string())
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
                client_config
                    .as_ref()
                    .map_or(self.connect_timeout, |c| c.connect_timeout)
                    .unwrap_or(DEFAULT_TIMEOUT),
            ))
            .build()?;

        let (result_text, status) = match client.request(http_method.into(), url).send().await {
            Ok(val) => self.validate_response(val, client_config).await?,
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

    use crate::db::tests::test_setup;
    use crate::prelude::*;

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
            http_status: Some(super::DEFAULT_EXPECTED_HTTP_STATUS),
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
}
