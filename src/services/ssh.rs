//! SSH-based service, SSH to a host and run a command

use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Stdio;

use crate::prelude::*;

#[derive(Debug, Deserialize, JsonSchema)]
/// SSH-based service, SSH to a host and run a command
pub struct SshService {
    /// Name of the service
    pub name: String,

    /// Command to run on the remote host
    pub command_line: String,

    /// Schedule for the service
    #[serde(with = "crate::serde::cron")]
    #[schemars(with = "String")]
    pub cron_schedule: Cron,

    /// Username to connect with
    pub username: Option<String>,

    /// SSH key to use
    pub ssh_key: Option<PathBuf>,
}

// TODO: look at using this instead of shelling out https://crates.io/crates/ssh-rs

#[async_trait]
impl ServiceTrait for SshService {
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        // ssh to the target host and run the command
        let start_time = chrono::Utc::now();
        let mut args: Vec<String> = vec![];

        if let Some(username) = &self.username {
            args.extend(vec!["-l".to_string(), format!("{}", username)]);
        }
        args.push(host.hostname.clone());

        if let Some(ssh_key) = &self.ssh_key {
            if !ssh_key.exists() {
                return Ok(CheckResult {
                    timestamp: start_time,
                    result_text: format!("SSH key not found: {}", ssh_key.display()),
                    status: ServiceStatus::Critical,
                    time_elapsed: chrono::Utc::now() - start_time,
                });
            }
            args.extend(vec![
                "-i".to_string(),
                ssh_key.to_string_lossy().to_string(),
            ]);
        }

        args.extend(
            self.command_line
                .split(' ')
                .map(String::from)
                .collect::<Vec<String>>(),
        );
        let child = tokio::process::Command::new("ssh")
            .args(args)
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| Error::Generic(err.to_string()))?;

        let res = child
            .wait_with_output()
            .await
            .map_err(|err| Error::Generic(err.to_string()))?;

        let time_elapsed = chrono::Utc::now() - start_time;

        if res.status != std::process::ExitStatus::from_raw(0) {
            return Ok(CheckResult {
                timestamp: start_time,
                result_text: String::from_utf8_lossy(&res.stderr).to_string(),
                status: ServiceStatus::Critical,
                time_elapsed,
            });
        }

        Ok(CheckResult {
            timestamp: start_time,
            result_text: "Ok".to_string(),
            status: ServiceStatus::Ok,
            time_elapsed,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;

    #[tokio::test]
    /// This will test the SshService and only run if you have the MAREMMA_TEST_SSH_HOST env var set
    async fn test_live_ssh_service() {
        let hostname = match std::env::var("MAREMMA_TEST_SSH_HOST") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("MAREMMA_TEST_SSH_HOST not set, skipping test");
                return;
            }
        };

        let service = super::SshService {
            name: hostname.clone(),
            command_line: "ls -lah .".to_string(),
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            username: None,
            ssh_key: None,
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
    fn test_parse_cliservice() {
        let service: super::SshService = match serde_json::from_str(
            r#" {
            "name": "local_lslah",
            "service_type": "ssh",
            "host_groups": ["local_lslah"],
            "command_line": "ls -lah /tmp",
            "cron_schedule": "* * * * *"
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
