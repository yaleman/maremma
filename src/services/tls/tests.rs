use serde_json::json;
use uuid::Uuid;

use crate::db::entities::host::test_host;
use crate::db::tests::test_setup;
use crate::services::tls::TlsService;

#[tokio::test]
async fn test_working_tls_service() {
    use crate::prelude::*;

    let (_, _) = test_setup().await.expect("Failed to set up test");

    let service = crate::services::tls::TlsService {
        name: "test".to_string(),
        cron_schedule: "0 0 * * * * *".parse().unwrap(),
        port: 443,
        expiry_critical: Some(1),
        expiry_warn: Some(3),
        timeout: None,
    };
    let host: entities::host::Model = entities::host::Model {
        check: crate::host::HostCheck::None,
        hostname: "example.com".to_string(),
        ..test_host()
    };
    let result = service.run(&host).await;
    dbg!(&result);
    assert!(result.is_ok());
    assert!(result.unwrap().status == ServiceStatus::Ok);
}

#[tokio::test]
#[cfg(feature = "test_badssl")]
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
        hostname: "expired.badssl.com".to_string(),
        ..test_host()
    };
    let result = service.run(&host).await;
    dbg!(&result);
    assert!(result.is_ok());
    assert!(result.unwrap().status == ServiceStatus::Critical);
}

#[tokio::test]
#[cfg(feature = "test_badssl")]
async fn test_wrong_cert_host_name() {
    use crate::prelude::*;

    let _ = setup_logging(true);
    let (_, _) = test_setup().await.expect("Failed to set up test");

    let service_def = serde_json::json! {{
        "name": "test",
        "cron_schedule": "0 0 * * *",
        "port": 443,
    }};

    let service: TlsService = serde_json::from_value(service_def).expect("Failed to parse service");
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
#[tokio::test]
async fn test_nxdomain() {
    use crate::prelude::*;

    let (_, _) = test_setup().await.expect("Failed to set up test");

    let service_def = serde_json::json! {{
        "name": "test",
        "cron_schedule": "0 0 * * *",
        "port": 443,
    }};

    let service: TlsService = serde_json::from_value(service_def).expect("Failed to parse service");
    let bad_hostname = "11.22.33.44.55.66.77.example.com".to_string();
    let host = entities::host::Model {
        name: bad_hostname.clone(),
        check: crate::host::HostCheck::None,
        id: Uuid::new_v4(),
        hostname: bad_hostname,
        config: json!({}),
    };
    let result = service.run(&host).await;
    dbg!(&result);
    assert!(result.is_ok());
    assert!(result.unwrap().status == ServiceStatus::Critical);
}

#[tokio::test]
async fn test_invalid_hostname() {
    use crate::prelude::*;

    let (_, _) = test_setup().await.expect("Failed to set up test");

    let service_def = serde_json::json! {{
        "name": "test",
        "cron_schedule": "0 0 * * *",
        "port": 443,
    }};

    let service: TlsService = serde_json::from_value(service_def).expect("Failed to parse service");
    let bad_hostname = "".to_string();
    let host = entities::host::Model {
        name: bad_hostname.clone(),
        check: crate::host::HostCheck::None,
        id: Uuid::new_v4(),
        hostname: bad_hostname,
        config: json!({}),
    };
    let result = service.run(&host).await;
    dbg!(&result);
    assert!(result.is_ok());
    assert!(result.unwrap().status == ServiceStatus::Critical);
}

#[tokio::test]
#[cfg(feature = "test_badssl")]
async fn test_tls_sha1_intermediate() {
    use crate::prelude::*;

    let _ = setup_logging(true);
    let (_, _) = test_setup().await.expect("Failed to set up test");

    let service_def = serde_json::json! {{
        "name": "test",
        "cron_schedule": "0 0 * * *",
        "port": 443,
    }};

    let service: TlsService = serde_json::from_value(service_def).expect("Failed to parse service");
    let bad_hostname = "sha1-intermediate.badssl.com".to_string();
    let host = entities::host::Model {
        name: bad_hostname.clone(),
        check: crate::host::HostCheck::None,
        id: Uuid::new_v4(),
        hostname: bad_hostname,
    };
    let result = service.run(&host).await;
    dbg!(&result);
    assert!(result.is_ok());
    assert!(result.unwrap().status == ServiceStatus::Critical);
}

#[tokio::test]
#[cfg(feature = "test_badssl")]
async fn test_tls_no_subject() {
    use crate::prelude::*;

    let _ = setup_logging(true);
    let (_, _) = test_setup().await.expect("Failed to set up test");

    let service_def = serde_json::json! {{
        "name": "test",
        "cron_schedule": "0 0 * * *",
        "port": 443,
    }};

    let service: TlsService = serde_json::from_value(service_def).expect("Failed to parse service");
    let bad_hostname = "no-subject.badssl.com".to_string();
    let host = entities::host::Model {
        name: bad_hostname.clone(),
        check: crate::host::HostCheck::None,
        id: Uuid::new_v4(),
        hostname: bad_hostname,
    };
    let result = service.run(&host).await;
    dbg!(&result);
    assert!(result.is_ok());
    assert!(result.unwrap().status == ServiceStatus::Critical);
}

// TODO: once we can generate arbitrary test certs, test for one that expires in x days

#[tokio::test]
async fn test_zero_port() {
    use crate::prelude::*;

    let (_, _) = test_setup().await.expect("Failed to set up test");

    let service_def = serde_json::json! {{
        "name": "test",
        "cron_schedule": "0 0 * * *",
        "port": 0,
    }};

    let service: TlsService = serde_json::from_value(service_def).expect("Failed to parse service");

    let bad_hostname = "no-subject.badssl.com".to_string();
    let host = entities::host::Model {
        name: bad_hostname.clone(),
        check: crate::host::HostCheck::None,
        id: Uuid::new_v4(),
        hostname: bad_hostname,
        config: json!({}),
    };
    let result = service.run(&host).await;
    dbg!(&result);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_timeout() {
    use crate::prelude::*;

    let (_, _) = test_setup().await.expect("Failed to set up test");

    let service_def = serde_json::json! {{
        "name": "test",
        "cron_schedule": "0 0 * * *",
        "port": 12345,
        "timeout" : Some(0),
    }};

    let service: TlsService = serde_json::from_value(service_def).expect("Failed to parse service");
    let bad_hostname = "example.com".to_string();
    let host = entities::host::Model {
        name: bad_hostname.clone(),
        check: crate::host::HostCheck::None,
        id: Uuid::new_v4(),
        hostname: bad_hostname,
        config: json!({}),
    };
    let result = service.run(&host).await;
    dbg!(&result);
    assert!(result.is_err());
}

#[test]
fn test_service_parser() {
    let mut extra_config = std::collections::HashMap::new();

    extra_config.insert("port".to_string(), json! {1234});
    let mut service = super::Service {
        id: Uuid::new_v4(),
        name: Some("Hello world".to_string()),
        description: None,
        host_groups: vec![],
        service_type: super::ServiceType::Tls,
        cron_schedule: "* * * * *".parse().expect("Failed to parse cron"),
        extra_config,
        config: Some(Box::new(TlsService {
            name: "tls_service".to_string(),
            cron_schedule: croner::Cron::new("* * * * *"),
            port: 1234,
            expiry_critical: Some(1),
            expiry_warn: Some(7),
            timeout: Some(5),
        })),
    };
    let _ = service.parse_config().expect("Failed to parse config!");
}

#[test]
fn test_failed_service_parser() {
    let mut service = super::Service {
        id: Uuid::new_v4(),
        name: Some("Hello world".to_string()),
        description: None,
        host_groups: vec![],
        service_type: super::ServiceType::Tls,
        cron_schedule: "* * * * *".parse().expect("Failed to parse cron"),
        extra_config: std::collections::HashMap::new(),
        config: Some(Box::new(TlsService {
            name: "tls_service".to_string(),
            cron_schedule: croner::Cron::new("* * * * *"),
            port: 1234,
            expiry_critical: Some(1),
            expiry_warn: Some(7),
            timeout: Some(5),
        })),
    };
    assert!(service.parse_config().is_err());
}
