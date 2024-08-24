//! Implements the one-shot command

use crate::cli::OneShotCmd;
use crate::prelude::*;
use crate::services::cli::CliService;
use crate::services::http::HttpService;
use crate::services::ping::PingService;
use crate::services::service_config_parse;
use crate::services::ssh::SshService;
use crate::services::tls::TlsService;

/// Because I'm fancy and silly
fn oneshot_uuid() -> Uuid {
    let mut oneshot_bytes: [u8; 16] = [0; 16];
    oneshot_bytes.copy_from_slice("--- one shot ---".as_bytes());
    Uuid::from_bytes(oneshot_bytes)
}

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

    let mut service_config: serde_json::Value = serde_json::from_str(&cmd.service_config)?;

    let service_config = match service_config.as_object_mut() {
        Some(obj) => {
            obj.insert("name".to_string(), "oneshot".to_string().into());
            obj.insert("cron_schedule".to_string(), "* * * * *".to_string().into());
            debug!("{:?}", obj);
            serde_json::to_value(obj)?
        }
        None => {
            return Err(Error::Configuration(
                "Service config must be a map of key-value pairs".to_string(),
            ))
        }
    };

    debug!("Service config: {:#?}", service_config);

    let service = service_config_parse(&oneshot_uuid().to_string(), &cmd.check, &service_config)?;

    if let Err(err) = service.validate() {
        error!("Failed to validate service configuration: {:#?}", err);
        return Err(err);
    }

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
