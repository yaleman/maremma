//! Basic ping service

use schemars::JsonSchema;
use tokio::net::lookup_host;

use crate::prelude::*;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
/// A service that pings things
pub struct PingService {
    #[serde(
        with = "crate::serde::cron"
    )]
    /// The cron schedule for this service
    #[schemars(with = "String")]
    pub cron_schedule: Cron,
}

#[async_trait]
impl ServiceTrait for PingService {
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        let start_time = chrono::Utc::now();
        let payload = [0; 8];

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
    use crate::setup_logging;

    use super::*;

    #[tokio::test]
    async fn test_ping_service_localhost() {
        let _ = setup_logging(true, true);
        let test_service = super::PingService {
            cron_schedule: Cron::new("* * * * *").parse().unwrap(),
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: "localhost".to_string(),
            check: crate::host::HostCheck::None,
        };
        let res = test_service.run(&host).await;
        dbg!(&res);
        assert!(res.is_ok());
    }
}
