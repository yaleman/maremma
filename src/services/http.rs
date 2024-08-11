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
#[derive(Debug, Deserialize)]
pub struct HttpService {
    pub name: String,
    #[serde(default)]
    pub run_in_shell: bool,
    #[serde(
        deserialize_with = "crate::serde::deserialize_croner_cron",
        serialize_with = "crate::serde::serialize_croner_cron"
    )]
    pub cron_schedule: Cron,

    #[serde(default)]
    pub http_method: HttpMethod,

    /// Defaults to nothing
    pub http_uri: Option<String>,

    /// Expected status code, defaults to 200
    pub http_status: Option<u16>,
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

        let client = reqwest::Client::new();

        let (result_text, status) = match client.request(self.http_method.into(), url).send().await
        {
            Ok(val) => {
                let expected_status_code = reqwest::StatusCode::from_u16(
                    self.http_status.unwrap_or(200),
                )
                .map_err(|_| {
                    Error::Generic(format!(
                        "Invalid status code {} in service check",
                        self.http_status.unwrap_or(200)
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
            run_in_shell: false,
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            http_method: crate::services::http::HttpMethod::Post,
            http_uri: None,
            http_status: None,
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

        assert!(Service::try_from(&json! {
            {
                "name": "test",
                "run_in_shell": false,
                "service_type": "http",
            }
        })
        .is_err());
    }
}
