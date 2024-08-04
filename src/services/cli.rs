use crate::prelude::*;
use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;

#[derive(Debug, Deserialize, Serialize)]
pub struct CliService {
    pub name: String,
    pub command_line: String,
    #[serde(default)]
    pub run_in_shell: bool,
    #[serde(
        deserialize_with = "crate::serde::deserialize_croner_cron",
        serialize_with = "crate::serde::serialize_croner_cron"
    )]
    pub cron_schedule: Cron,
}

#[async_trait]
impl ServiceTrait for CliService {
    async fn run(&self, _host: &Host) -> Result<ServiceStatus, Error> {
        // run the command line and capture the exit code and stdout
        let mut cmd_split = self.command_line.split(" ");
        let cmd = match cmd_split.next() {
            Some(c) => c,
            None => return Err(Error::Generic("No command specified!".to_string())),
        };
        let args = cmd_split.collect::<Vec<&str>>();
        // let stdout = Stdio::piped();
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

        if res.status != std::process::ExitStatus::from_raw(0) {
            return Ok(ServiceStatus::Critical);
        }

        Ok(ServiceStatus::Ok)
    }
}

#[cfg(test)]
mod tests {
    use crate::host::Host;
    use crate::services::ServiceTrait;

    #[tokio::test]
    async fn test_cliservice() {
        let service = super::CliService {
            name: "test".to_string(),
            command_line: "ls -lah .".to_string(),
            run_in_shell: false,
            cron_schedule: "@hourly".parse().expect("Failed to parse cron schedule"),
        };
        let res = service
            .run(&Host::new(
                "hello.example.com".to_string(),
                crate::host::HostCheck::Ping,
            ))
            .await;
        assert_eq!(service.name, "test".to_string());
        assert_eq!(res.is_ok(), true);
    }

    #[test]
    fn test_parse_cliservice() {
        let service: super::CliService = serde_json::from_str(
            r#" {
            "name": "local_lslah",
            "type": "cli",
            "host_groups": ["local_lslah"],
            "command_line": "ls -lah /tmp",
            "cron_schedule": "* * * * *"
        }"#,
        )
        .expect("Failed to parse!");
        assert_eq!(service.name, "local_lslah".to_string());
    }
}
