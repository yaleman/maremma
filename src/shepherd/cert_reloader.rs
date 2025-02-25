//! Reloads the web server when the cert or key changes

use super::prelude::*;

/// Task to check if any certificates have changed
pub(crate) struct CertReloaderTask {
    tx: tokio::sync::mpsc::Sender<WebServerControl>,
    config: SendableConfig,
    cert_time: DateTime<Utc>,
    key_time: DateTime<Utc>,
}

/// Get the last modified time of a file
#[instrument(level = "debug")]
fn get_file_time(file: &std::path::Path) -> Result<DateTime<Utc>, Error> {
    let file = file.canonicalize().inspect_err(|err| {
        error!(
            "Failed to get canonical path for {} error={:?}",
            file.display(),
            err
        )
    })?;

    let metadata = file.metadata()?;
    let modified = metadata.modified()?;
    Ok(DateTime::<Utc>::from(modified))
}

#[instrument(level = "debug", skip(config))]
async fn get_file_times(config: SendableConfig) -> Result<(DateTime<Utc>, DateTime<Utc>), Error> {
    let config_reader = config.read().await;

    let cert_time = get_file_time(&config_reader.cert_file).inspect_err(|err| {
        error!(
            "Failed to get metadata for TLS cert at {} {:?}",
            config_reader.cert_file.display(),
            err
        )
    })?;
    let key_time = get_file_time(&config_reader.cert_key).inspect_err(|err| {
        error!(
            "Failed to get metadata for TLS key at {} {:?}",
            config_reader.cert_key.display(),
            err
        )
    })?;
    Ok((cert_time, key_time))
}

impl CertReloaderTask {
    pub(crate) async fn new(
        tx: tokio::sync::mpsc::Sender<WebServerControl>,
        config: SendableConfig,
    ) -> Result<Self, Error> {
        // get the time for the cert
        let config_reader = config.read().await;

        if !config_reader.cert_file.exists() {
            return Err(Error::Configuration(format!(
                "Couldn't find cert file at {}",
                config_reader.cert_file.display()
            )));
        }
        if !config_reader.cert_key.exists() {
            return Err(Error::Configuration(format!(
                "Couldn't find cert key file at {}",
                config_reader.cert_key.display()
            )));
        }

        let (cert_time, key_time) = get_file_times(config.clone()).await?;

        Ok(Self {
            tx,
            config: config.clone(),
            cert_time,
            key_time,
        })
    }
}

#[async_trait]
impl CronTaskTrait for CertReloaderTask {
    async fn run(&mut self, _db: Arc<RwLock<DatabaseConnection>>) -> Result<(), Error> {
        let (cert_time, key_time) = get_file_times(self.config.clone()).await?;

        if cert_time != self.cert_time || key_time != self.key_time {
            info!("TLS cert or key has changed, reloading...");
            self.cert_time = cert_time;
            self.key_time = key_time;
            if self.tx.send(WebServerControl::Reload).await.is_err() {
                error!("Tried to tell the web server to reload but couldn't!");
                return Err(Error::IoError(
                    "Tried to tell the web server to reload but couldn't!".to_string(),
                ));
            }
        }
        self.cert_time = cert_time;
        self.key_time = key_time;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::RwLock;

    use crate::config::Configuration;
    use crate::prelude::test_setup;

    use super::*;
    #[tokio::test]
    async fn test_get_file_time() {
        let (_db, config) = test_setup().await.expect("Failed to set up tests");

        // this should fail because it's set to a non-existent file in the test config
        assert!(get_file_times(config.clone()).await.is_err());

        // make some tempfiles so we know things exist
        let tempdir = tempfile::tempdir().expect("Failed to create tempdir");
        let cert_file = tempdir.path().join("cert_file");
        std::fs::write(&cert_file, "test").expect("Failed to write to cert file");
        let cert_key = tempdir.path().join("cert_key");
        std::fs::write(&cert_key, "test").expect("Failed to write to key file");

        get_file_time(tempdir.path().join("YeahNah").as_path())
            .expect_err("This should definitely fail!");
        get_file_time(std::path::Path::new("Cargo.toml"))
            .expect("Failed to get file time for Cargo.toml, which should exist");

        // good cert, bad key
        config.write().await.cert_file = cert_file.clone();
        assert!(get_file_times(config.clone()).await.is_err());
        // good cert, good key
        config.write().await.cert_key = cert_key.clone();
        assert!(get_file_times(config.clone()).await.is_ok());
        // good key, bad cert
        config.write().await.cert_file = tempdir.path().join("nope");
        assert!(get_file_times(config.clone()).await.is_err());
    }

    #[tokio::test]
    async fn test_cert_reloader_task() {
        let (db, _config) = test_setup().await.expect("Failed to set up tests");
        let bad_config = Configuration {
            cert_file: std::path::PathBuf::from("bad_cert_file"),
            cert_key: std::path::PathBuf::from("bad_cert_key"),
            ..Default::default()
        };

        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        let mut task = CertReloaderTask {
            tx,
            config: Arc::new(RwLock::new(bad_config)),
            cert_time: chrono::Utc::now(),
            key_time: chrono::Utc::now(),
        };

        let res = task.run(db.clone()).await;

        dbg!(&res);
        assert!(res.is_err());

        // make some tempfiles so we know things exist
        let tempdir = tempfile::tempdir().expect("Failed to create tempdir");
        let cert_file = tempdir.path().join("cert_file");
        std::fs::write(&cert_file, "test").expect("Failed to write to cert file");
        let cert_key = tempdir.path().join("cert_key");
        std::fs::write(&cert_key, "test").expect("Failed to write to key file");

        let good_config = Configuration {
            cert_file: cert_file.clone(),
            cert_key: cert_key.clone(),
            ..Default::default()
        };

        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        let mut task = CertReloaderTask {
            tx,
            config: Arc::new(RwLock::new(good_config)),
            cert_time: chrono::Utc::now(),
            key_time: chrono::Utc::now(),
        };

        let res = task.run(db).await;
        let _ = rx.recv().await;

        dbg!(&res);
        assert!(res.is_ok());
    }
}
