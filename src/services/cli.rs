//! CLI-based service checks

use schemars::JsonSchema;

use super::prelude::*;
use crate::prelude::*;
use std::process::Stdio;

#[derive(Debug, Deserialize, Serialize, clap::Parser, JsonSchema)]
/// A service that runs on the command line, typically on the Maremma server
pub struct CliService {
    /// Name of the service
    pub name: String,
    /// Hostname for overlaying on the service
    pub hostname: Option<String>,
    /// Command line to run, you can use #HOSTNAME# to substitute the hostname
    pub command_line: String,
    #[serde(default)]
    /// If we should run the command in a shell
    pub run_in_shell: bool,
    #[serde(with = "crate::serde::cron")]
    #[schemars(with = "String")]
    /// Cron schedule for the service
    pub cron_schedule: Cron,
    /// Add random jitter in 0..n seconds to the check
    pub jitter: Option<u16>,
}

impl ConfigOverlay for CliService {
    fn overlay_host_config(&self, value: &Map<String, Json>) -> Result<Box<Self>, MaremmaError> {
        let cron_schedule = self.extract_cron(value, "cron_schedule", &self.cron_schedule)?;
        let hostname = self.extract_value(value, "hostname", &self.hostname)?;
        let name = self.extract_string(value, "name", &self.name);
        let command_line = self.extract_string(value, "command_line", &self.command_line);

        Ok(Box::new(Self {
            name,
            hostname,
            cron_schedule,
            command_line,
            run_in_shell: self.extract_bool(value, "run_in_shell", self.run_in_shell),
            jitter: self.extract_value(value, "jitter", &self.jitter)?,
        }))
    }
}

#[async_trait]
impl ServiceTrait for CliService {
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, MaremmaError> {
        let start_time = chrono::Utc::now();
        // run the command line and capture the exit code and stdout

        let config = self.overlay_host_config(&self.get_host_config(&self.name, host)?)?;

        let hostname = match &config.hostname {
            Some(h) => h.to_owned(),
            None => host.hostname.to_owned(),
        };

        let command_line = config.command_line.replace("#HOSTNAME#", &hostname);

        let mut command = if config.run_in_shell {
            let mut command = tokio::process::Command::new("/bin/sh");
            command.arg("-c").arg(&command_line);
            command
        } else {
            let mut cmd_split = command_line.split_whitespace();
            let cmd = cmd_split
                .next()
                .ok_or_else(|| MaremmaError::InvalidInput("No command specified".to_string()))?;

            let which_cmd = which::which(cmd).map_err(|err| {
                MaremmaError::CommandNotFound(format!("Couldn't find {cmd}, error: {err}"))
            })?;

            if !which_cmd.exists() {
                return Err(MaremmaError::CommandNotFound(format!(
                    "Command not found: {}",
                    which_cmd.display()
                )));
            }

            let mut command = tokio::process::Command::new(which_cmd);
            command.args(cmd_split);
            command
        };

        let child = command
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| MaremmaError::Generic(err.to_string()))?;

        let res = child
            .wait_with_output()
            .await
            .map_err(|err| MaremmaError::Generic(err.to_string()))?;

        let time_elapsed = chrono::Utc::now() - start_time;

        let mut combined = res.stdout;
        combined.extend(res.stderr);
        let status = MonitoringPluginExit::from(res.status.code()).into();

        Ok(CheckResult {
            timestamp: Utc::now(),
            result_text: String::from_utf8_lossy(&combined)
                .to_string()
                .replace(r#"\\n"#, " "),
            status,
            time_elapsed,
        })
    }

    fn as_json_pretty(&self, host: &entities::host::Model) -> Result<String, MaremmaError> {
        let config = self.overlay_host_config(&self.get_host_config(&self.name, host)?)?;
        Ok(serde_json::to_string_pretty(&config)?)
    }

    fn jitter_value(&self) -> u32 {
        self.jitter.unwrap_or(0) as u32
    }
}

#[cfg(test)]
mod tests {
    use entities::host::test_host;
    use std::str::FromStr;

    use crate::prelude::*;

    #[tokio::test]
    async fn test_cliservice() {
        let service = super::CliService {
            name: "test".to_string(),
            hostname: None,
            command_line: "ls -lah .".to_string(),
            run_in_shell: false,
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            jitter: None,
        };
        let host = entities::host::Model {
            check: crate::host::HostCheck::None,
            ..test_host()
        };

        let res = service.run(&host).await;
        assert_eq!(service.name, "test".to_string());
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn shell_service_preserves_quoting_hostname_and_warning_status() {
        let service = super::CliService {
            name: "shell".to_string(),
            hostname: None,
            command_line: "printf '%s' '#HOSTNAME# quoted value'; exit 1".to_string(),
            run_in_shell: true,
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            jitter: None,
        };
        let host = entities::host::Model {
            check: crate::host::HostCheck::None,
            ..test_host()
        };

        let result = service.run(&host).await.expect("Shell check failed to run");
        assert_eq!(result.status, ServiceStatus::Warning);
        assert_eq!(result.result_text, "test_host_hostname quoted value");
    }

    #[tokio::test]
    async fn shell_service_preserves_stdout_stderr_and_performance_data() {
        let service = super::CliService {
            name: "shell-output".to_string(),
            hostname: None,
            command_line: "printf 'CRITICAL | value=7'; printf ' diagnostic' >&2; exit 2"
                .to_string(),
            run_in_shell: true,
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
            jitter: None,
        };

        let result = service
            .run(&test_host())
            .await
            .expect("Shell check failed to run");

        assert_eq!(result.status, ServiceStatus::Critical);
        assert_eq!(result.result_text, "CRITICAL | value=7 diagnostic");
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
            Err(err) => panic!("Failed to parse service: {err:?}"),
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
            cron_schedule: Cron::from_str("@hourly").expect("Failed to parse cron"),
            extra_config: HashMap::from_iter([("hello".to_string(), json!("world"))]),
            config: None
        }
        .parse_config()
        .is_err());
    }
}
