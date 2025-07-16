//! SSH-based service, SSH to a host and run a command

use std::num::NonZeroU16;
use std::path::PathBuf;

use super::prelude::*;
use crate::prelude::*;

fn serialize_password<S>(password: &Option<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    if let Some(password) = password {
        // mask the password
        let password_mask = "*".repeat(password.len());
        serializer.serialize_str(&password_mask)
    } else {
        serializer.serialize_none()
    }
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
/// SSH-based service, SSH to a host and run a command
pub struct SshService {
    /// Name of the service
    pub name: String,

    /// Command to run on the remote host
    pub command_line: String,

    // Port to connect to, defaults to 22
    port: Option<NonZeroU16>,

    /// Schedule for the service
    #[serde(with = "crate::serde::cron")]
    #[schemars(with = "String")]
    pub cron_schedule: Cron,

    /// Username to connect with
    pub username: String,

    /// SSH key to use, keys with passphrases are not currently supported (because of ssh-rs... so far)
    pub private_key: Option<PathBuf>,

    /// If you're bad, but you have to. Won't try this is the private key is set.
    #[serde(serialize_with = "serialize_password")]
    pub password: Option<String>,

    /// Expected exit code (Defaults to 0)
    pub exit_code: Option<u32>,

    /// Connection timeout (seconds), not runtime-timeout
    pub timeout: Option<u32>,

    /// Add random jitter in 0..n seconds to the check
    pub jitter: Option<u16>,
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
            jitter: None,
        }
    }
}

impl ConfigOverlay for SshService {
    fn overlay_host_config(&self, value: &Map<String, Json>) -> Result<Box<Self>, Error> {
        Ok(Box::new(Self {
            name: self.extract_string(value, "name", &self.name),
            cron_schedule: self.extract_cron(value, "cron_schedule", &self.cron_schedule)?,
            command_line: self
                .extract_string(value, "command_line", &self.command_line)
                .to_string(),
            port: self.extract_value(value, "port", &self.port)?,
            username: self
                .extract_string(value, "username", &self.username)
                .to_string(),
            private_key: self.extract_value(value, "private_key", &self.private_key)?,
            password: self.extract_value(value, "password", &self.password)?,
            exit_code: self.extract_value(value, "exit_code", &self.exit_code)?,
            timeout: self.extract_value(value, "timeout", &self.timeout)?,
            jitter: self.extract_value(value, "jitter", &self.jitter)?,
        }))
    }
}

#[async_trait]
impl ServiceTrait for SshService {
    /// ssh to the target host and run the command
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        let start_time = chrono::Utc::now();

        let config = self.overlay_host_config(&self.get_host_config(&self.name, host)?)?;

        let mut session = ssh::create_session().username(&config.username);

        // adds the SSH key if we have one, checking first that we have the key
        if let Some(ssh_key) = &config.private_key {
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
        } else if let Some(password) = &config.password {
            debug!("Using password for connection");
            session = session.password(password);
        }

        let target = format!(
            "{}:{}",
            host.hostname.clone(),
            config.port.map(u16::from).unwrap_or(22)
        );

        let mut session = session
            .connect(&target)
            .map_err(|err| {
                error!("Failed to connect to {}", target);
                Error::Generic(err.to_string())
            })?
            .run_local();

        debug!("Running ssh command: {:?}", &config.command_line);

        let mut exec = session.open_exec().map_err(|err| {
            error!("Failed to open exec: {:?}", err);
            Error::Generic(err.to_string())
        })?;
        exec.exec_command(&config.command_line).map_err(|err| {
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

        let status = match exit_status == config.exit_code.unwrap_or(0) {
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
        // TODO: this should overlay the host config too
        if self.private_key.is_none() && self.password.is_none() {
            return Err(Error::Configuration(
                "No SSH key or password provided, auth is going to fail!".to_string(),
            ));
        }
        Ok(())
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
    use std::path::PathBuf;

    use super::*;
    use crate::db::tests::test_setup;

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
            command_line: "ls -lah /".to_string(),
            private_key: Some(private_key),
            username,
            ..Default::default()
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: hostname.clone(),
            check: crate::host::HostCheck::None,
            config: json!({}),
        };

        let res = service.run(&host).await;
        dbg!(&res);
        assert_eq!(service.name, hostname);
        assert!(res.is_ok());
        assert!(res.expect("Failed to run").status == ServiceStatus::Ok);
    }

    #[tokio::test]
    async fn test_intentionally_failing_ssh_service() {
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
            command_line: "exit 1".to_string(),
            private_key: Some(private_key),
            username,
            exit_code: Some(1),
            ..Default::default()
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: hostname.clone(),
            check: crate::host::HostCheck::None,
            config: json!({}),
        };

        let res = service.run(&host).await;

        dbg!(&res);
        assert_eq!(service.name, hostname);
        assert!(res.is_ok());

        assert!(res.expect("Failed to run").status == ServiceStatus::Ok);
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
            Err(err) => panic!("Failed to parse service: {err:?}"),
            Ok(val) => val,
        };
        assert_eq!(service.name, "local_lslah".to_string());

        assert!(service.validate().is_ok());

        // test parsing broken service
        let mut bad_service = Service {
            name: Some("test".to_string()),
            service_type: ServiceType::Ssh,
            id: Default::default(),
            description: None,
            host_groups: vec![],
            cron_schedule: Cron::new("@hourly").parse().expect("Failed to parse cron"),
            extra_config: HashMap::from_iter([("hello".to_string(), json!("world"))]),
            config: None,
        };

        assert!(bad_service.parse_config().is_err());

        let bad_service_missing_auth: super::SshService = serde_json::from_str(
            r#" {
            "name": "local_lslah",
            "service_type": "ssh",
            "host_groups": ["local_lslah"],
            "command_line": "ls -lah /tmp",
            "cron_schedule": "* * * * *",
            "username" : "testuser"
        }"#,
        )
        .expect("Failed to parse bad_service_missing_auth to SshService from JSON");

        assert!(bad_service_missing_auth.validate().is_err());

        let good_service: super::SshService = serde_json::from_value(json!({
            "name": "check_ntp_time",
            "service_type": "ssh",
            "host_groups": ["check_ntp_time"],
            "command_line": "/usr/lib/nagios/plugins/check_ntp_time -H localhost",
            "cron_schedule": "*/15 * * * *",
            "username": "maremma",
            "private_key": "/.ssh/maremma"
        }))
        .expect("Failed to parse good_service");

        assert_eq!(good_service.validate(), Ok(()));
    }

    #[test]
    fn test_serialize_password() {
        #[derive(Serialize)]
        struct SecurePassword {
            #[serde(serialize_with = "serialize_password")]
            password: Option<String>,
        }

        let secure = SecurePassword {
            password: Some("hunter2".to_string()),
        };

        let serialized = serde_json::to_string(&secure).expect("Failed to serialize password");
        assert_eq!(serialized, r#"{"password":"*******"}"#);

        let empty_secure = SecurePassword { password: None };

        let empty_serialized =
            serde_json::to_string(&empty_secure).expect("Failed to serialize empty password");
        assert_eq!(empty_serialized, r#"{"password":null}"#);
    }
}
