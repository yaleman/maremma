use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::prelude::*;

pub type ServiceChecks = Arc<RwLock<HashMap<String, ServiceCheck>>>;

#[derive(Debug)]
pub struct ServiceCheck {
    pub host_id: Arc<String>,
    pub service_id: String,
    pub status: ServiceStatus,
    pub last_check: DateTime<Utc>,
    last_updated: DateTime<Utc>,
    check_id: String,
}

impl ServiceCheck {
    pub fn new(host_id: Arc<String>, service_id: String) -> Self {
        let check_id = service_check_id(&host_id, &service_id);
        Self {
            host_id,
            service_id,
            status: ServiceStatus::Pending,
            last_check: DateTime::<Utc>::from_timestamp(0, 0)
                .expect("Failed to create 0 timestamp"),
            last_updated: chrono::Utc::now(),
            check_id,
        }
    }

    /// Is this due to be run?
    pub fn is_due(
        &self,
        config: &Configuration,
        now: Option<DateTime<Utc>>,
    ) -> Result<bool, Error> {
        let cron = self.get_cron(config)?;
        let next_runtime = cron
            .find_next_occurrence(&self.last_check, true)
            .map_err(|err| Error::Generic(format!("{:?}", err)))?;
        Ok(next_runtime < now.unwrap_or(chrono::Utc::now()))
    }

    pub fn urgent(&mut self) {
        self.status = ServiceStatus::Urgent;
    }

    pub fn checkout(&mut self) {
        self.last_updated = chrono::Utc::now();
        self.status = ServiceStatus::Checking;
        debug!("Checking out {:?}", self);
    }

    pub fn checkin(&mut self, status: ServiceStatus) {
        self.last_check = chrono::Utc::now();
        self.last_updated = self.last_check;
        self.status = status;
        debug!("Checked in {:?}", self);
    }

    /// A hash of the host ID and service ID
    pub fn check_id(&self) -> &str {
        self.check_id.as_ref()
    }

    pub fn get_cron(&self, config: &Configuration) -> Result<Cron, Error> {
        let service = config
            .service_table
            .get(&*self.service_id)
            .ok_or(Error::ServiceNotFound)?;
        Ok(service.cron_schedule.clone())
    }
}

pub fn service_check_id(host_id: impl ToString, service_id: &str) -> String {
    sha256::digest(&format!("{}-{}", host_id.to_string(), service_id))
}
