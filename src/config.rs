use std::collections::HashMap;
use std::path::PathBuf;

use sea_orm::{QueryOrder, QuerySelect};

use crate::db::entities;
use crate::db::entities::service_check::Model;
use crate::host::fakehost::FakeHost;
use crate::host::{Host, HostCheck};
use crate::prelude::*;

pub type ServiceTable = HashMap<Uuid, Service>;

fn default_database_file() -> String {
    "maremma.sqlite".to_string()
}

fn default_listen_address() -> String {
    "127.0.0.1".to_string()
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Configuration {
    #[serde(default = "default_database_file")]
    pub database_file: String,

    #[serde(default = "default_listen_address")]
    pub listen_address: String,

    //// Defaults to 8888
    pub listen_port: Option<u16>,

    pub hosts: HashMap<String, Host>,

    #[serde(default)]
    pub local_services: FakeHost,

    // This is something we need to deserialize later because it's messy
    #[serde(skip_serializing)]
    pub services: Option<serde_json::Value>,
}

impl Configuration {
    pub async fn new(filename: Option<PathBuf>) -> Result<Self, Error> {
        let filename = filename.unwrap_or(PathBuf::from(DEFAULT_CONFIG_FILE));
        if !filename.exists() {
            return Err(Error::ConfigFileNotFound(
                filename.to_string_lossy().to_string(),
            ));
        }
        debug!("Loading config from {:?}", filename);
        Self::new_from_string(&tokio::fs::read_to_string(filename).await?).await
    }

    pub async fn new_from_string(config: &str) -> Result<Self, Error> {
        let mut res: Configuration = serde_json::from_str(config)?;

        if !res.local_services.services.is_empty() {
            res.hosts.insert(
                LOCAL_SERVICE_HOST_NAME.to_string(),
                Host::new(LOCAL_SERVICE_HOST_NAME.to_string(), HostCheck::None),
            );
        }
        Ok(res)
    }
    #[cfg(test)]
    pub async fn load_test_config() -> Self {
        let mut res: Configuration = serde_json::from_str(
            &tokio::fs::read_to_string("maremma.example.json")
                .await
                .expect("Failed to load exampleconfig"),
        )
        .expect("Failed to parse example config");

        if !res.local_services.services.is_empty() {
            res.hosts.insert(
                LOCAL_SERVICE_HOST_NAME.to_string(),
                Host::new(LOCAL_SERVICE_HOST_NAME.to_string(), HostCheck::None),
            );
        }
        res
    }

    pub async fn run_check(&self, _next_check_id: &str) -> Result<(String, ServiceStatus), Error> {
        todo!()
        // let check = self.service_checks.read().await;
        // let check = check
        //     .get(next_check_id)
        //     .ok_or(Error::ServiceCheckNotFound(next_check_id.to_string()))?;

        // let host = self
        //     .hosts
        //     .iter()
        //     .find(|host| host.host_id() == check.host_id)
        //     .ok_or(Error::HostNotFound((*check.host_id).clone()))?;

        // let service = self
        //     .service_table
        //     .get(&check.service_id)
        //     .ok_or(Error::ServiceNotFound)?;
        // if let Some(config) = &service.config {
        //     match config.run(host).await {
        //         Ok(val) => Ok((host.hostname(), val)),
        //         Err(err) => Err(err),
        //     }
        // } else {
        //     Err(Error::ServiceConfigNotFound(next_check_id.to_string()))
        // }
    }

    /// find the next time we need to wake up
    pub async fn find_next_wakeup(&self) -> DateTime<Utc> {
        // let mut next_wakeup: Option<DateTime<Utc>> = None;

        // for (_id, check) in self.service_checks.read().await.iter() {
        //     if let Ok(cron) = check.get_cron(self) {
        //         if let Ok(next_runtime) = cron.find_next_occurrence(&check.last_check, false) {
        //             match next_wakeup {
        //                 Some(wakeup) => {
        //                     if next_runtime < wakeup {
        //                         next_wakeup = Some(next_runtime);
        //                     }
        //                 }
        //                 None => {
        //                     next_wakeup = Some(next_runtime);
        //                 }
        //             }
        //         }
        //     }
        // }
        // next_wakeup.unwrap_or(chrono::Utc::now() + TimeDelta::seconds(1))
        chrono::Utc::now() + TimeDelta::seconds(1)
    }

    pub fn get_host(&self, _host_id: &str) -> Option<&Host> {
        // self.hosts.iter().find(|host| *host.host_id() == host_id)
        None
        // TODO
    }

    pub fn get_service(&self, _service_id: &str) -> Option<&Service> {
        // self.service_table.get(service_id)
        None // TODO
    }
}

/// Get the next service check to run, returns
pub async fn get_next_service_check(db: &DatabaseConnection) -> Result<Option<Model>, Error> {
    let urgent = entities::service_check::Entity::find()
        .filter(entities::service_check::Column::Status.eq(ServiceStatus::Urgent))
        // oldest-last-updated is the most urgent
        .order_by_asc(entities::service_check::Column::LastUpdated)
        .limit(1)
        .all(db)
        .await?;

    if let Some(model) = urgent.into_iter().next() {
        return Ok(Some(model));
    }
    // prioritize pending

    if let Some(res) = entities::service_check::Entity::find()
        .filter(entities::service_check::Column::Status.ne(ServiceStatus::Disabled))
        .all(db)
        .await?
        .into_iter()
        .next()
    {
        return Ok(Some(res));
    }

    Ok(entities::service_check::Entity::find()
        .filter(entities::service_check::Column::Status.ne(ServiceStatus::Disabled))
        .all(db)
        .await?
        .into_iter()
        .next())
}

#[cfg(test)]
mod tests {
    // use std::path::PathBuf;

    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::config::{get_next_service_check, Configuration};
    use crate::setup_logging;
    // use crate::host::{Host, HostCheck};

    #[tokio::test]
    async fn test_config_new() {
        let config = serde_json::json! {{
            "hosts": {
                "foo.bar" : {
                    "hostname" : "foo.bar"
                }
            }
        }}
        .to_string();
        let config = Configuration::new_from_string(&config).await.unwrap();
        assert_eq!(config.hosts.len(), 1);
    }
    #[tokio::test]
    async fn test_next_service_check() {
        let _ = setup_logging(true);
        let db = Arc::new(
            crate::db::test_connect()
                .await
                .expect("Failed to connect to database"),
        );

        let configuration =
            crate::config::Configuration::new(Some(PathBuf::from("maremma.example.json")))
                .await
                .expect("Failed to load config");

        crate::db::update_db_from_config(db.clone(), &configuration)
            .await
            .unwrap();

        let next_check = get_next_service_check(&db).await.unwrap();
        dbg!(&next_check);
        assert!(next_check.is_some());
    }
}
