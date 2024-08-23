//! CLI-based service checks

use schemars::JsonSchema;

use crate::prelude::*;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Stdio;

#[derive(Debug, Deserialize, Serialize, clap::Parser, JsonSchema)]
/// A service that runs on the command line, typically on the Maremma server
pub struct CliService {
    /// Name of the service
    pub name: String,
    /// Command line to run
    pub command_line: String,
    #[serde(default)]
    /// If we should run the command in a shell
    pub run_in_shell: bool,
    #[serde(with = "crate::serde::cron")]
    #[schemars(with = "String")]
    /// Cron schedule for the service
    pub cron_schedule: Cron,
}

#[async_trait]
impl ServiceTrait for CliService {
    async fn run(&self, _host: &entities::host::Model) -> Result<CheckResult, Error> {
        let start_time = chrono::Utc::now();
        // run the command line and capture the exit code and stdout
        let mut cmd_split = self.command_line.split(" ");
        let cmd = match cmd_split.next() {
            Some(c) => c,
            None => return Err(Error::Generic("No command specified!".to_string())),
        };

        if !(PathBuf::from(cmd)).exists() {
            // check if the command exists
            return Ok(CheckResult {
                timestamp: chrono::Utc::now(),
                result_text: format!("Command not found: {}", cmd),
                status: ServiceStatus::Critical,
                time_elapsed: chrono::Utc::now() - start_time,
            });
        }

        let args = cmd_split.collect::<Vec<&str>>();

        let child = tokio::process::Command::new(cmd)
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
                timestamp: chrono::Utc::now(),
                result_text: String::from_utf8_lossy(&res.stderr).to_string(),
                status: ServiceStatus::Critical,
                time_elapsed,
            });
        }

        Ok(CheckResult {
            timestamp: chrono::Utc::now(),
            result_text: String::from_utf8_lossy(&res.stdout).to_string(),
            status: ServiceStatus::Ok,
            time_elapsed,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;

    #[tokio::test]
    async fn test_cliservice() {
        let service = super::CliService {
            name: "test".to_string(),
            command_line: "ls -lah .".to_string(),
            run_in_shell: false,
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
        };
        let host = entities::host::Model {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            hostname: "localhost".to_string(),
            check: crate::host::HostCheck::None,
        };

        let res = service.run(&host).await;
        assert_eq!(service.name, "test".to_string());
        assert!(res.is_ok());
    }

    #[test]
    fn test_parse_cliservice() {
        let service: super::CliService = match serde_json::from_str(
            r#" {
            "name": "local_lslah",
            "service_type": "cli",
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
            service_type: ServiceType::Cli,
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
