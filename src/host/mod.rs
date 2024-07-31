use std::sync::Arc;

use crate::prelude::*;

pub mod fakehost;
pub mod kube;
pub mod ssh;

#[derive(Deserialize, Serialize, Debug)]
pub struct Host {
    pub name: String,
    #[serde(default = "Default::default")]
    pub check: HostCheck,

    hostname: Option<String>,

    #[serde(default = "Default::default")]
    pub host_groups: Vec<String>,

    #[serde(skip)]
    host_id: Arc<String>,
}

impl Host {
    pub fn new(name: impl ToString, check: HostCheck) -> Self {
        let host_id = Arc::new(sha256::digest(&format!(
            "{}:{:?}",
            name.to_string(),
            &check
        )));
        debug!("Creating host: {} with id: {}", name.to_string(), host_id);
        Self {
            name: name.to_string(),
            hostname: None,
            check,
            host_groups: vec![],
            host_id,
        }
    }

    pub fn with_hostname(self, hostname: impl ToString) -> Self {
        Self {
            hostname: Some(hostname.to_string()),
            ..self
        }
    }

    pub fn with_host_groups(self, host_groups: Vec<String>) -> Self {
        Self {
            host_groups,
            ..self
        }
    }

    pub fn host_id(&self) -> Arc<String> {
        if self.host_id.is_empty() {
            Arc::new(generate_host_id(&self.name, &self.check))
        } else {
            self.host_id.clone()
        }
    }

    pub fn hostname(&self) -> String {
        self.hostname.clone().unwrap_or_else(|| self.name.clone())
    }
}

fn generate_host_id(name: impl ToString, check: &HostCheck) -> String {
    sha256::digest(&format!("{}:{:?}", name.to_string(), check))
}

#[derive(Deserialize, Debug, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HostCheck {
    /// No checks done
    None,
    /// Checks by pinging the host
    #[default]
    Ping,
    /// Checks by trying to SSH to the host
    SshHost,
    /// Checks we can connect to the Kubernetes API
    KubeHost {
        api_hostname: String,
        #[serde(default = "kube::kube_port_default")]
        api_port: u16,
    },
}

#[async_trait]
pub trait GenericHost
where
    Self: std::marker::Sized,
{
    async fn check_up(&self) -> Result<bool, crate::errors::Error>;

    fn name(&self) -> String;

    fn id(&self) -> String;

    fn try_from_config(config: serde_json::Value) -> Result<Self, Error>
    where
        Self: Sized;
}
