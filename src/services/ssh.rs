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

#[async_trait]
impl ServiceTrait for SshService {
    async fn run(&self, host: &Host) -> Result<ServiceStatus, Error> {
        // ssh to the target host and run the command
        let mut args = vec![host.hostname()];
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

        if res.status != std::process::ExitStatus::from_raw(0) {
            return Ok(ServiceStatus::Critical);
        }

        Ok(ServiceStatus::Ok)
    }
}
