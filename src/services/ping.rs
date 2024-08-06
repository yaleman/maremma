use tokio::net::lookup_host;

use crate::prelude::*;

#[derive(Debug, Deserialize)]
pub struct PingService {
    pub name: String,
    pub host: String,
    #[serde(deserialize_with = "crate::serde::deserialize_croner_cron")]
    pub cron_schedule: Cron,
}

#[async_trait]
impl ServiceTrait for PingService {
    async fn run(&self, host: &entities::host::Model) -> Result<(String, ServiceStatus), Error> {
        let payload = [0; 8];

        let hostname = lookup_host(format!("{}:80", host.hostname.clone()))
            .await?
            .next()
            .ok_or(Error::DNSFailed)?;

        let (_packet, duration) = match surge_ping::ping(hostname.ip(), &payload).await {
            Ok((packet, duration)) => (packet, duration),
            Err(err) => return Err(Error::Generic(err.to_string())),
        };
        let res = format!("OK: Ping to {} took {}ms", host.name, duration.as_millis());

        Ok((res, ServiceStatus::Ok))
    }
}

#[cfg(test)]
mod tests {
    use crate::setup_logging;

    use super::*;

    #[tokio::test]
    async fn test_ping_service_localhost() {
        let _ = setup_logging(true);
        let test_service = super::PingService {
            name: "test".to_string(),
            host: "localhost".to_string(),
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
