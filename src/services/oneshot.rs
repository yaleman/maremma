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

fn export_config(cmd: &OneShotCmd) -> (String, String) {
    let schema: RootSchema = match cmd.check {
        ServiceType::Cli => schema_for!(CliService),
        ServiceType::Ssh => schema_for!(SshService),
        ServiceType::Ping => schema_for!(PingService),
        ServiceType::Http => schema_for!(HttpService),
        ServiceType::Tls => schema_for!(TlsService),
    };
    (
        format!("Dumping schema for {:?}", cmd.check),
        // because we're not relying on external things and we tested before release, right?
        #[allow(clippy::expect_used)]
        serde_json::to_string_pretty(&schema)
            .expect("Somehow we failed to serialize a validated config?"),
    )
}

/// Runs a single check and exits
pub async fn run_oneshot(
    cmd: OneShotCmd,
    _config: Arc<RwLock<Configuration>>,
) -> Result<(), Error> {
    if cmd.show_config {
        let (msg, config) = export_config(&cmd);
        eprintln!("{}", msg);
        println!("{}", config);
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

    service.validate()?;

    let host = entities::host::Model {
        id: Uuid::new_v4(),
        name: cmd.hostname.clone(),
        hostname: cmd.hostname.clone(),
        check: crate::host::HostCheck::None,
        config: json!({}),
    };
    #[cfg(not(test))]
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
    #[cfg(test)]
    {
        debug!("Host: {:#?}", host);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::Iterable;

    use crate::cli::SharedOpts;
    use crate::db::tests::test_setup;

    use super::*;

    #[tokio::test]

    async fn test_run_oneshot() {
        let (_, config) = test_setup().await.expect("Failed to set up test");

        let cmd = OneShotCmd {
            sharedopts: SharedOpts::default(),
            check: ServiceType::Ping,
            hostname: "localhost".to_string(),
            service_config: json! {{"cron_schedule" : "@hourly"}}.to_string(),
            show_config: false,
        };

        let res = run_oneshot(cmd, config.clone()).await;
        dbg!(&res);
        assert!(res.is_ok());

        let cmd = OneShotCmd {
            sharedopts: SharedOpts::default(),
            check: ServiceType::Ping,
            hostname: "localhost".to_string(),
            service_config: json! {{}}.to_string(),
            show_config: false,
        };

        let res = run_oneshot(cmd, config).await;
        dbg!(&res);
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_show_config_oneshot() {
        let (_, config) = test_setup().await.expect("Failed to set up test");
        // this sample config should fill everything, yeeet
        let service_config = json! {{
            "cron_schedule" : "@hourly",
            "username" : "test",
            "password" : "test",
            "command_line" : "echo",
            "port" : 22
        }}
        .to_string();

        for check in ServiceType::iter() {
            let cmd = OneShotCmd {
                sharedopts: SharedOpts::default(),
                check,
                hostname: "localhost".to_string(),
                service_config: service_config.clone(),
                show_config: true,
            };

            export_config(&cmd);

            run_oneshot(cmd, config.clone())
                .await
                .expect("failed to run oneshot");
        }
    }

    #[test]
    fn test_oneshot_uuid() {
        let uuid = oneshot_uuid();
        assert_eq!(uuid.to_string(), "2d2d2d20-6f6e-6520-7368-6f74202d2d2d");
    }

    #[tokio::test]
    async fn test_invalid_oneshot() {
        let (_, config) = test_setup().await.expect("Failed to set up test");
        let service_config = json!("{}").to_string();
        let cmd = OneShotCmd {
            sharedopts: SharedOpts::default(),
            check: ServiceType::Ping,
            hostname: "localhost".to_string(),
            service_config,
            show_config: false,
        };
        let res = run_oneshot(cmd, config.clone()).await;

        assert_eq!(
            res,
            Err(Error::Configuration(
                "Service config must be a map of key-value pairs".to_string()
            ))
        );
        // ssh expects a password or key amongst other things
        let service_config =
            json!({"username":"lol", "command_line" : "lol", "foo": "bar"}).to_string();
        let cmd = OneShotCmd {
            sharedopts: SharedOpts::default(),
            check: ServiceType::Ssh,
            hostname: "localhost".to_string(),
            service_config,
            show_config: false,
        };
        let res = run_oneshot(cmd, config).await;
        dbg!(&res);
        assert_eq!(
            res,
            Err(Error::Configuration(
                "No SSH key or password provided, auth is going to fail!".to_string()
            ))
        );
    }
}
