use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::prelude::*;

pub type ServiceChecks = Arc<RwLock<HashMap<String, ServiceCheck>>>;

#[derive(Debug)]
pub struct ServiceCheck {
    pub id: Uuid,
    pub host_id: Arc<Uuid>,
    pub service_id: Arc<Uuid>,
    pub status: ServiceStatus,
    pub last_check: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
}

impl ServiceCheck {
    pub fn new(host_id: Arc<Uuid>, service_id: Arc<Uuid>) -> Self {
        #[allow(clippy::expect_used)]
        let last_check =
            DateTime::<Utc>::from_timestamp(0, 0).expect("Failed to create 0 timestamp");
        Self {
            id: Uuid::new_v4(),
            host_id,
            service_id,
            status: ServiceStatus::Pending,
            last_check,
            last_updated: chrono::Utc::now(),
        }
    }

    /// Is this due to be run?
    pub fn is_due(
        &self,
        _config: &Configuration,
        _now: Option<DateTime<Utc>>,
    ) -> Result<bool, Error> {
        // let next_runtime = cron
        //     .find_next_occurrence(&self.last_check, false)
        //     .map_err(|err| Error::Generic(format!("{:?}", err)))?;
        // Ok(next_runtime < now.unwrap_or(chrono::Utc::now()))
        todo!()
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
    pub fn check_id(&self) -> &Uuid {
        self.id.as_ref()
    }
}

pub fn generate_service_check_id(host_id: &Uuid, service_id: &Uuid) -> String {
    sha256::digest(&format!("{}-{}", host_id.hyphenated(), service_id))
}
