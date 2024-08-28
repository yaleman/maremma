//! Basic ping service

use tokio::net::lookup_host;

use super::prelude::*;
use crate::prelude::*;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
/// A service that pings things
pub struct PingService {
    /// Name of the service
    pub name: String,
    #[serde(with = "crate::serde::cron")]
    /// The cron schedule for this service
    #[schemars(with = "String")]
    pub cron_schedule: Cron,
}

impl ConfigOverlay for PingService {
    fn overlay_host_config(&self, value: &Map<String, Json>) -> Result<Box<Self>, Error> {
        let name = match value.get("name") {
            Some(val) => val.as_str().map(String::from).ok_or_else(|| {
                Error::Configuration("Failed to parse name from host config".to_string())
            })?,
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

        Ok(Box::new(Self {
            name,
            cron_schedule,
        }))
    }
}

#[async_trait]
impl ServiceTrait for PingService {
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        let start_time = chrono::Utc::now();
        let payload = [0; 8];

        let _config = self.overlay_host_config(&self.get_host_config(&self.name, host)?)?;

        let hostname = lookup_host(format!("{}:80", host.hostname.clone()))
            .await?
            .next()
            .ok_or(Error::DnsFailed)?;

        let (_packet, duration) = match surge_ping::ping(hostname.ip(), &payload).await {
            Ok((packet, duration)) => (packet, duration),
            Err(err) => return Err(Error::Generic(err.to_string())),
        };
        let res = format!("OK: Ping to {} took {}ms", host.name, duration.as_millis());

        Ok(CheckResult {
            timestamp: start_time,
            result_text: res,
            status: ServiceStatus::Ok,
            time_elapsed: chrono::Duration::from_std(duration)
                .map_err(|err| Error::Generic(err.to_string()))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::log::setup_logging;

    use super::*;

    #[tokio::test]
    async fn test_ping_service_localhost() {
        let _ = setup_logging(true, true);
        let test_service = super::PingService {
            name: "test".to_string(),
            cron_schedule: Cron::new("* * * * *").parse().unwrap(),
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: "localhost".to_string(),
            check: crate::host::HostCheck::None,
            config: json!({}),
        };
        let res = test_service.run(&host).await;
        dbg!(&res);
        assert!(res.is_ok());
    }
}
