//! CLI-based service checks

use schemars::JsonSchema;

use super::prelude::*;
use crate::prelude::*;
use std::os::unix::process::ExitStatusExt;
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
    fn overlay_host_config(&self, value: &Map<String, Json>) -> Result<Box<Self>, Error> {
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
    async fn run(&self, host: &entities::host::Model) -> Result<CheckResult, Error> {
        let start_time = chrono::Utc::now();
        // run the command line and capture the exit code and stdout

        let config = self.overlay_host_config(&self.get_host_config(&self.name, host)?)?;

        let hostname = match &config.hostname {
            Some(h) => h.to_owned(),
            None => host.hostname.to_owned(),
        };

        let command_line = config.command_line.replace("#HOSTNAME#", &hostname);

        let mut cmd_split = command_line.split(" ");
        let cmd = match cmd_split.next() {
            Some(c) => c,
            None => return Err(Error::Generic("No command specified!".to_string())),
        };

        let which_cmd = which::which(cmd).map_err(|err| Error::CommandNotFound(err.to_string()))?;

        if !which_cmd.exists() {
            // check if the command exists
            return Err(Error::CommandNotFound(format!(
                "Command not found: {}",
                cmd
            )));
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
            let mut combined = res.stderr.to_vec();
            combined.extend(res.stdout);
            return Ok(CheckResult {
                timestamp: chrono::Utc::now(),
                result_text: String::from_utf8_lossy(&combined)
                    .to_string()
                    .replace(r#"\\n"#, " "),
                status: ServiceStatus::Critical,
                time_elapsed,
            });
        }

        Ok(CheckResult {
            timestamp: chrono::Utc::now(),
            result_text: String::from_utf8_lossy(&res.stdout)
                .to_string()
                .replace(r#"\\n"#, " "),
            status: ServiceStatus::Ok,
            time_elapsed,
        })
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
    use entities::host::test_host;

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
