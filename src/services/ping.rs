// use std::net::IpAddr;
// use std::os::unix::net::SocketAddr;
// use std::os::unix::process::ExitStatusExt;
// use std::process::Stdio;

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
    async fn run(&self, _host: &Host) -> Result<ServiceStatus, Error> {
        // TODO: implement this

        Ok(ServiceStatus::Ok)
    }
}
