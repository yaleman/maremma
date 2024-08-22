//! Implements the one-shot command

use crate::cli::OneShotCmd;
use crate::prelude::*;
use crate::services::cli::CliService;
use crate::services::http::HttpService;
use crate::services::ping::PingService;
use crate::services::service_config_parse;
use crate::services::ssh::SshService;
use crate::services::tls::TlsService;

/// Runs a single check and exits
pub async fn run_oneshot(cmd: OneShotCmd, _config: Arc<Configuration>) -> Result<(), Error> {
    if cmd.show_config {
        let schema: RootSchema = match cmd.check {
            ServiceType::Cli => schema_for!(CliService),
            ServiceType::Ssh => schema_for!(SshService),
            ServiceType::Ping => schema_for!(PingService),
            ServiceType::Http => schema_for!(HttpService),
            ServiceType::Tls => schema_for!(TlsService),
        };
        eprintln!("Dumping schema for {:?}", cmd.check);
        println!("{}", serde_json::to_string_pretty(&schema).unwrap());
        return Ok(());
    }

    let service_config: serde_json::Value = serde_json::from_str(&cmd.service_config)?;

    let service = service_config_parse(&Uuid::new_v4().to_string(), &cmd.check, &service_config)?;

    let host = entities::host::Model {
        id: Uuid::new_v4(),
        name: cmd.hostname.clone(),
        hostname: cmd.hostname.clone(),
        check: crate::host::HostCheck::None,
    };
    match service.run(&host).await {
        Ok(res) => {
            info!("Result: {:#?}", res);
            Ok(())
        }
        Err(err) => {
            error!("Failed to run service: {:#?}", err);
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::Iterable;

    use crate::cli::SharedOpts;

    use super::*;

    #[tokio::test]
    #[cfg(feature = "test_badssl")]
    async fn test_run_oneshot() {
        let cmd = OneShotCmd {
            sharedopts: SharedOpts::default(),
            check: ServiceType::Ping,
            hostname: "example.com".to_string(),
            service_config: json! {{"cron_schedule" : "@hourly"}}.to_string(),
            show_config: false,
        };

        let config = Arc::new(Configuration::default());

        let res = run_oneshot(cmd, config).await;
        dbg!(&res);
        assert!(res.is_ok());

        let cmd = OneShotCmd {
            sharedopts: SharedOpts::default(),
            check: ServiceType::Ping,
            hostname: "example.com".to_string(),
            service_config: json! {{}}.to_string(),
            show_config: false,
        };

        let config = Arc::new(Configuration::default());

        let res = run_oneshot(cmd, config).await;
        dbg!(&res);
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_show_config_oneshot() {
        for check in ServiceType::iter() {
            let cmd = OneShotCmd {
                sharedopts: SharedOpts::default(),
                check,
                hostname: "example.com".to_string(),
                service_config: json! {{"cron_schedule" : "@hourly"}}.to_string(),
                show_config: true,
            };

            run_oneshot(cmd, Arc::new(Configuration::default()))
                .await
                .unwrap();
        }
    }
}
