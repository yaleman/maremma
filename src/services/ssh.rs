use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;

use crate::prelude::*;

#[derive(Debug, Deserialize)]
pub struct SshService {
    pub name: String,
    pub command_line: String,
    #[serde(deserialize_with = "crate::serde::deserialize_croner_cron")]
    pub cron_schedule: Cron,
}

// TODO: look at using this instead of shelling out https://crates.io/crates/ssh-rs

#[async_trait]
impl ServiceTrait for SshService {
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        // ssh to the target host and run the command
        let start_time = chrono::Utc::now();
        let mut args = vec![host.hostname.clone()];

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
                result_text: "CRITICAL".to_string(),
                status: ServiceStatus::Critical,
                time_elapsed,
            });
        }

        Ok(CheckResult {
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
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: hostname.clone(),
            check: crate::host::HostCheck::None,
        };

        let res = service.run(&host).await;
        assert_eq!(service.name, hostname);
        assert_eq!(res.is_ok(), true);
    }

    #[test]
    fn test_parse_cliservice() {
        let service: super::SshService = match serde_json::from_str(
            r#" {
            "name": "local_lslah",
            "type": "ssh",
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
            type_: ServiceType::Ssh,
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
