//! SSH-based service, SSH to a host and run a command

use std::path::PathBuf;

use crate::prelude::*;

#[derive(Debug, Deserialize, JsonSchema)]
/// SSH-based service, SSH to a host and run a command

pub struct SshService {
    /// Name of the service
    pub name: String,

    /// Command to run on the remote host
    pub command_line: String,

    // Port to connect to, defaults to 22
    port: Option<u16>,

    /// Schedule for the service
    #[serde(with = "crate::serde::cron")]
    #[schemars(with = "String")]
    pub cron_schedule: Cron,

    /// Username to connect with
    pub username: String,

    /// SSH key to use, keys with passphrases are not currently supported (because of ssh-rs... so far)
    pub private_key: Option<PathBuf>,

    // TODO: add test for a non-default exit code
    /// Expected exit code (Defaults to 0)
    pub exit_code: Option<u32>,

    /// If you're bad, but you have to
    pub password: Option<String>,

    /// Connection timeout (seconds), not runtime-timeout
    pub timeout: Option<u32>,
}

impl Default for SshService {
    #[allow(clippy::expect_used)] // Because we're setting a default value and know in our hearts it's OK.
    fn default() -> Self {
        Self {
            name: "default name".to_string(),
            command_line: "echo 'hello world.'".to_string(),
            cron_schedule: Cron::new("@hourly")
                .parse()
                .expect("Failed to parse default cron schedule"),
            port: None,
            username: "maremma".to_string(),
            private_key: None,
            exit_code: None,
            password: None,
            timeout: None,
        }
    }
}

#[async_trait]
impl ServiceTrait for SshService {
    /// ssh to the target host and run the command
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        let start_time = chrono::Utc::now();

        let mut session = ssh::create_session().username(&self.username);

        // adds the SSH key if we have one, checking first that we have the key
        if let Some(ssh_key) = &self.private_key {
            if !ssh_key.exists() {
                return Ok(CheckResult {
                    timestamp: start_time,
                    result_text: format!("SSH key not found: {}", ssh_key.display()),
                    status: ServiceStatus::Critical,
                    time_elapsed: chrono::Utc::now() - start_time,
                });
            }

            debug!("Using SSH key {} for connection", ssh_key.display());
            session = session.private_key_path(ssh_key);
        } else if let Some(password) = &self.password {
            debug!("Using password for connection");
            session = session.password(password);
        }

        let target = format!("{}:{}", host.hostname.clone(), self.port.unwrap_or(22));

        let mut session = session
            .connect(&target)
            .map_err(|err| {
                error!("Failed to connect to {}", target);
                Error::Generic(err.to_string())
            })?
            .run_local();

        debug!("Running ssh command: {:?}", &self.command_line);

        let mut exec = session.open_exec().map_err(|err| {
            error!("Failed to open exec: {:?}", err);
            Error::Generic(err.to_string())
        })?;
        exec.exec_command(&self.command_line).map_err(|err| {
            error!("Failed to send SSH command: {:?}", err);
            Error::Generic(err.to_string())
        })?;

        let output = exec.get_output().map_err(|err| {
            error!("Failed to get output: {:?}", err);
            Error::Generic(err.to_string())
        })?;

        let result_text = String::from_utf8_lossy(&output).to_string();
        let exit_status = exec.exit_status().map_err(|err| {
            error!("Failed to get exit status: {:?}", err);
            Error::Generic(err.to_string())
        })?;

        let time_elapsed = chrono::Utc::now() - start_time;

        let status = match exit_status == self.exit_code.unwrap_or(0) {
            false => ServiceStatus::Critical,
            true => ServiceStatus::Ok,
        };

        Ok(CheckResult {
            timestamp: start_time,
            result_text,
            status,
            time_elapsed,
        })
    }

    /// Validate the configuration
    fn validate(&self) -> Result<(), Error> {
        if self.private_key.is_none() && self.password.is_none() {
            return Err(Error::Generic(
                "No SSH key or password provided, auth is going to fail!".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::db::tests::test_setup;
    use crate::prelude::*;

    #[tokio::test]
    /// This will test the SshService and only run if you have the MAREMMA_TEST_SSH_HOST env var set
    async fn test_live_ssh_service() {
        let _ = test_setup().await.expect("Failed to set up test harness");

        let hostname = match std::env::var("MAREMMA_TEST_SSH_HOST") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("MAREMMA_TEST_SSH_HOST not set, skipping test");
                return;
            }
        };
        let username = match std::env::var("MAREMMA_TEST_SSH_USERNAME") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("MAREMMA_TEST_SSH_USERNAME not set, skipping test");
                return;
            }
        };
        let private_key = match std::env::var("MAREMMA_TEST_SSH_KEY") {
            Ok(val) => PathBuf::from(val),
            Err(_) => {
                eprintln!("MAREMMA_TEST_SSH_KEY not set, skipping test");
                return;
            }
        };

        debug!(
            "Running test with hostname={} username={} private_key={}",
            hostname,
            username,
            private_key.display()
        );
        let service = super::SshService {
            name: hostname.clone(),
            command_line: "ls -lah .".to_string(),
            private_key: Some(private_key),
            username,
            ..Default::default()
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: hostname.clone(),
            check: crate::host::HostCheck::None,
        };

        let res = service.run(&host).await;
        assert_eq!(service.name, hostname);
        assert!(res.is_ok());
    }

    #[test]
    fn test_parse_ssh_service() {
        let service: super::SshService = match serde_json::from_str(
            r#" {
            "name": "local_lslah",
            "service_type": "ssh",
            "host_groups": ["local_lslah"],
            "command_line": "ls -lah /tmp",
            "cron_schedule": "* * * * *",
            "username" : "testuser",
            "password" : "testpassword"
        }"#,
        ) {
            Err(err) => panic!("Failed to parse service: {:?}", err),
            Ok(val) => val,
        };
        assert_eq!(service.name, "local_lslah".to_string());

        // test parsing broken service
        assert!(Service {
            name: Some("test".to_string()),
            service_type: ServiceType::Ssh,
            id: Default::default(),
            description: None,
            host_groups: vec![],
            cron_schedule: Cron::new("@hourly").parse().expect("Failed to parse cron"),
            extra_config: HashMap::from_iter([("hello".to_string(), json!("world"))]),
            config: None
        }
        .parse_config()
        .is_err());
    }
}
