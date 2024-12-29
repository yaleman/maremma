//! Basic ping service

use surge_ping::SurgeError;
use tokio::net::lookup_host;

use super::prelude::*;
use crate::prelude::*;

const DEFAULT_COUNT: u8 = 3;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
/// A service that pings things
pub struct PingService {
    /// Name of the service
    pub name: String,
    #[serde(with = "crate::serde::cron")]
    /// The cron schedule for this service
    #[schemars(with = "String")]
    pub cron_schedule: Cron,

    /// Add random jitter in 0..n seconds to the check
    pub jitter: Option<u16>,

    /// Number of pings to check, defaults to 3
    pub count: Option<u8>,

    /// Optionally configure the address to ping
    #[serde(default)]
    pub address: Option<String>,

    /// Minimum successes required for the check to be considered successful, defaults to the same as count
    pub required_successful: Option<u8>,
}

impl PingService {
    /// Get the count field with the default
    fn get_count(&self) -> u8 {
        self.count.unwrap_or(DEFAULT_COUNT)
    }

    /// Get the minimum number of successes required for the check to be considered successful, but won't be larger than the count
    fn get_required_successful(&self) -> u8 {
        let res = self.required_successful.unwrap_or(self.get_count());
        if res > self.get_count() {
            self.get_count()
        } else {
            res
        }
    }
}

impl ConfigOverlay for PingService {
    fn overlay_host_config(&self, value: &Map<String, Json>) -> Result<Box<Self>, Error> {
        Ok(Box::new(Self {
            name: self.extract_string(value, "name", &self.name),
            address: self.extract_value(value, "address", &self.address)?,
            cron_schedule: self.extract_cron(value, "cron_schedule", &self.cron_schedule)?,
            jitter: self.extract_value(value, "jitter", &self.jitter)?,
            count: self.extract_value(value, "count", &self.count)?,
            required_successful: self.extract_value(
                value,
                "required_successful",
                &self.required_successful,
            )?,
        }))
    }
}

#[async_trait]
impl ServiceTrait for PingService {
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        let start_time = chrono::Utc::now();

        let config = self.overlay_host_config(&self.get_host_config(&self.name, host)?)?;

        let target = match config.address {
            Some(ref addr) => addr.clone(),
            None => host.hostname.clone(),
        };

        let hostname = lookup_host(format!("{}:80", target))
            .await?
            .next()
            .ok_or(Error::DnsFailed)?;

        let results = (0..self.get_count())
            .map(|_| tokio::spawn(surge_ping::ping(hostname.ip(), &[0; 8])))
            .collect::<Vec<_>>();

        // check the results and ensure all three are OK
        let mut total_duration = std::time::Duration::new(0, 0);
        let mut success_count = 0;

        for (index, result) in results.into_iter().enumerate() {
            match result.await {
                Ok(Ok((_, dur))) => {
                    total_duration += dur;
                    success_count += 1;
                }
                Ok(Err(err)) => {
                    match err {
                        SurgeError::Timeout { .. } => {
                            debug!("Ping {} timed out: {}", index, err.to_string());
                        }
                        _ => {
                            return Err(Error::Generic(err.to_string()));
                        }
                    }
                    return Err(Error::Generic(err.to_string()));
                }
                Err(err) => {
                    return Err(Error::Generic(format!("Running task failed: {}", err)));
                }
            }
        }

        if success_count == self.get_required_successful() {
            let avg_duration = total_duration / success_count as u32;
            Ok(CheckResult {
                timestamp: start_time,
                result_text: format!(
                    "OK: Ping to {} took {}ms on average",
                    host.name,
                    avg_duration.as_millis()
                ),
                status: ServiceStatus::Ok,
                time_elapsed: chrono::Utc::now() - start_time,
            })
        } else {
            Err(Error::Generic(format!(
                "CRITICAL: Ping failed: {} successful, {} failed",
                success_count,
                3 - success_count
            )))
        }
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
    use crate::log::setup_logging;

    use super::*;

    #[tokio::test]
    async fn test_ping_service_localhost() {
        let _ = setup_logging(true, true);
        let test_service = super::PingService {
            name: "test".to_string(),
            cron_schedule: Cron::new("* * * * *").parse().unwrap(),
            jitter: None,
            count: Some(5),
            required_successful: None,
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
