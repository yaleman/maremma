use crate::db::tests::test_setup;
use crate::setup_logging;

#[tokio::test]
async fn test_working_tls_service() {
    use crate::prelude::*;

    let _ = setup_logging(true);
    let (_, _) = test_setup().await.expect("Failed to set up test");

    let service = crate::services::tls::TlsService {
        name: "test".to_string(),
        cron_schedule: "0 0 * * * * *".parse().unwrap(),
        port: 443,
        expiry_critical: Some(1),
        expiry_warn: Some(3),
        timeout: None,
    };
    let host = entities::host::Model {
        name: "badssl.com".to_string(),
        check: crate::host::HostCheck::None,
        id: Uuid::new_v4(),
        hostname: "badssl.com".to_string(),
    };
    let result = service.run(&host).await;
    dbg!(&result);
    assert!(result.is_ok());
    assert!(result.unwrap().status == ServiceStatus::Ok);
}

#[tokio::test]
async fn test_expired_tls_service() {
    use crate::prelude::*;

    let _ = setup_logging(true);
    let (_, _) = test_setup().await.expect("Failed to set up test");

    let service = crate::services::tls::TlsService {
        name: "test".to_string(),
        cron_schedule: "0 0 * * * * *".parse().unwrap(),
        port: 443,
        expiry_critical: Some(30),
        expiry_warn: Some(60),
        timeout: None,
    };
    let host = entities::host::Model {
        name: "expired.badssl.com".to_string(),
        check: crate::host::HostCheck::None,
        id: Uuid::new_v4(),
        hostname: "expired.badssl.com".to_string(),
    };
    let result = service.run(&host).await;
    dbg!(&result);
    assert!(result.is_ok());
    assert!(result.unwrap().status == ServiceStatus::Critical);
}

#[tokio::test]
async fn test_wrong_name() {
    use crate::prelude::*;

    let _ = setup_logging(true);
    let (_, _) = test_setup().await.expect("Failed to set up test");

    let service = crate::services::tls::TlsService {
        name: "test".to_string(),
        cron_schedule: "0 0 * * * * *".parse().unwrap(),
        port: 443,
        expiry_critical: None,
        expiry_warn: None,
        timeout: None,
    };
    let host = entities::host::Model {
        name: "wrong.host.badssl.com".to_string(),
        check: crate::host::HostCheck::None,
        id: Uuid::new_v4(),
        hostname: "wrong.host.badssl.com".to_string(),
    };
    let result = service.run(&host).await;
    dbg!(&result);
    assert!(result.is_ok());
    assert!(result.unwrap().status == ServiceStatus::Critical);
}

// TODO: once we can generate arbitrary test certs, test for one that expires in x days
