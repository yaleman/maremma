use crate::prelude::*;

#[derive(Debug, Deserialize, Default, Copy, Clone)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

impl From<HttpMethod> for reqwest::Method {
    fn from(value: HttpMethod) -> Self {
        match value {
            HttpMethod::Get => Self::GET,
            HttpMethod::Post => Self::POST,
            HttpMethod::Put => Self::PUT,
            HttpMethod::Delete => Self::DELETE,
            HttpMethod::Patch => Self::PATCH,
        }
    }
}

fn default_true() -> bool {
    true
}

const DEFAULT_TIMEOUT: u64 = 10;
const DEFAULT_HTTP_STATUS: u16 = 200;

#[derive(Debug, Deserialize)]
pub struct HttpService {
    pub name: String,
    #[serde(
        deserialize_with = "crate::serde::deserialize_croner_cron",
        serialize_with = "crate::serde::serialize_croner_cron"
    )]
    pub cron_schedule: Cron,

    /// Defaults to GET
    #[serde(default)]
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
}

#[async_trait]
impl ServiceTrait for HttpService {
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        let start_time = chrono::Utc::now();

        let url = format!(
            "https://{}/{}",
            host.hostname,
            &self.http_uri.clone().unwrap_or("".to_string())
        );

        let client = reqwest::ClientBuilder::new()
            .danger_accept_invalid_certs(!self.validate_tls) // invert it
            .connect_timeout(std::time::Duration::from_secs(
                self.connect_timeout.unwrap_or(DEFAULT_TIMEOUT),
            ))
            .build()?;

        let (result_text, status) = match client.request(self.http_method.into(), url).send().await
        {
            Ok(val) => {
                let expected_status_code =
                    reqwest::StatusCode::from_u16(self.http_status.unwrap_or(DEFAULT_HTTP_STATUS))
                        .map_err(|_| {
                            Error::Generic(format!(
                                "Invalid status code {} in service check",
                                self.http_status.unwrap_or(DEFAULT_HTTP_STATUS)
                            ))
                        })?;
                if val.status() != expected_status_code {
                    (
                        format!(
                            "CRITICAL: Expected status code {}, got {}",
                            expected_status_code,
                            val.status()
                        ),
                        ServiceStatus::Critical,
                    )
                } else {
                    ("OK".to_string(), ServiceStatus::Ok)
                }
            }
            Err(err) => (format!("CRITICAL: {:?}", err), ServiceStatus::Critical),
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

    use crate::prelude::*;

    #[tokio::test]
    async fn test_httpservice() {
        let service = super::HttpService {
            name: "test".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_method: crate::services::http::HttpMethod::Post,
            http_uri: None,
            http_status: None,
            validate_tls: true,
            connect_timeout: Some(5),
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: "example.com".to_string(),
            check: crate::host::HostCheck::None,
        };

        let res = service.run(&host).await;
        assert_eq!(service.name, "test".to_string());
        assert_eq!(res.is_ok(), true);
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
    async fn test_skip_tls_verify() {
        let service = super::HttpService {
            name: "test".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_method: crate::services::http::HttpMethod::Get,
            http_uri: None,
            http_status: Some(super::DEFAULT_HTTP_STATUS),
            validate_tls: false,
            connect_timeout: Some(5),
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: "untrusted-root.badssl.com".to_string(),
            check: crate::host::HostCheck::None,
        };

        let res = service.run(&host).await;
        assert_eq!(service.name, "test".to_string());
        assert_eq!(res.is_ok(), true);
        assert_eq!(res.unwrap().status, ServiceStatus::Ok);

        let service = super::HttpService {
            name: "test".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_method: crate::services::http::HttpMethod::Get,
            http_uri: None,
            http_status: None,
            validate_tls: false,
            connect_timeout: Some(5),
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: "expired.badssl.com".to_string(),
            check: crate::host::HostCheck::None,
        };

        let res = service.run(&host).await;
        assert_eq!(service.name, "test".to_string());
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
