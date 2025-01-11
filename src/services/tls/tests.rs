use sea_orm::EntityTrait;
use serde_json::json;

use uuid::Uuid;

use crate::db::entities;
use crate::db::entities::host::test_host;
use crate::db::tests::test_setup;
use crate::services::tls::TlsService;
use crate::tests::testcontainers::TestContainer;
use crate::tests::tls_utils::TestCertificateBuilder;

#[tokio::test]
async fn test_working_tls_service() {
    use crate::prelude::*;
    use crate::tests::tls_utils::TestCertificateBuilder;

    let _ = test_setup().await.expect("Failed to set up test");

    let certs = TestCertificateBuilder::new()
        .with_name("localhost")
        .with_expiry((chrono::Utc::now() + chrono::TimeDelta::days(30)).timestamp())
        .with_issue_time((chrono::Utc::now() - chrono::TimeDelta::days(30)).timestamp())
        .build();

    let test_container = TestContainer::new(&certs, "test_working_tls_service").await;

    let service = crate::services::tls::TlsService {
        name: "test".to_string(),
        cron_schedule: "0 0 * * * * *".parse().unwrap(),
        port: test_container
            .tls_port
            .try_into()
            .expect("Failed to convert port"),
        expiry_critical: Some(0),
        expiry_warn: Some(3),
        timeout: None,
        jitter: None,
    };
    let host: entities::host::Model = entities::host::Model {
        check: crate::host::HostCheck::None,
        hostname: "localhost".to_string(),
        config: json!({
            "port": test_container.tls_port,
            "cron_schedule" : "* * * * *",
            "expiry_critical": 0,
            "expiry_warn" : 5,
        }),
        ..test_host()
    };

    dbg!(&host);
    let result = service.run(&host).await;
    dbg!(&result);
    assert!(result.is_ok());
    assert!(result.unwrap().status == ServiceStatus::Ok);
}

#[tokio::test]
async fn test_expired_tls_service() {
    use crate::prelude::*;
    use crate::tests::tls_utils::TestCertificateBuilder;

    let _ = test_setup().await.expect("Failed to set up test");

    let certs = TestCertificateBuilder::new()
        .with_name("localhost")
        .with_expiry((chrono::Utc::now() - chrono::TimeDelta::days(30)).timestamp())
        .build();

    let test_container = TestContainer::new(&certs, "test_expired_tls_service").await;

    let service = crate::services::tls::TlsService {
        name: "localhost".to_string(),
        cron_schedule: "0 0 * * * * *".parse().unwrap(),
        port: test_container
            .tls_port
            .try_into()
            .expect("Failed to convert port"),
        expiry_critical: Some(30),
        expiry_warn: Some(60),
        timeout: None,
        jitter: None,
    };
    let host = entities::host::Model {
        name: "localhost".to_string(),
        check: crate::host::HostCheck::None,
        hostname: "localhost".to_string(),
        ..test_host()
    };
    let result = service.run(&host).await;
    dbg!(&result);
    assert!(result.is_ok());
    assert!(result.unwrap().status == ServiceStatus::Critical);
}

#[tokio::test]
async fn test_wrong_cert_host_name() {
    use crate::prelude::*;
    use crate::tests::tls_utils::TestCertificateBuilder;

    let _ = test_setup().await.expect("Failed to set up test");

    let certs = TestCertificateBuilder::new()
        .with_name("this.should.fail")
        .with_expiry((chrono::Utc::now() - chrono::TimeDelta::days(30)).timestamp())
        .build();

    let test_container = TestContainer::new(&certs, "test_wrong_cert_host_name").await;

    let service_def = serde_json::json! {{
        "name": "test",
        "cron_schedule": "0 0 * * *",
        "port": test_container.tls_port,
    }};

    let service: TlsService = serde_json::from_value(service_def).expect("Failed to parse service");
    let host = entities::host::Model {
        name: "localhost".to_string(),
        check: crate::host::HostCheck::None,
        id: Uuid::new_v4(),
        hostname: "localhost".to_string(),
        config: json!({}),
    };
    let result = service.run(&host).await;
    dbg!(&result);
    assert!(result.is_ok());
    assert!(result.unwrap().status == ServiceStatus::Critical);
}

#[tokio::test]
async fn test_nxdomain() {
    use crate::prelude::*;

    let _ = test_setup().await.expect("Failed to set up test");

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

    let _ = test_setup().await.expect("Failed to set up test");

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
async fn test_tls_sha1_intermediate() {
    use crate::prelude::*;
    use crate::tests::tls_utils::TestCertificateBuilder;

    let _ = test_setup().await.expect("Failed to set up test");

    let certs = TestCertificateBuilder::new()
        .with_name("localhost")
        .with_sha1_intermediate()
        .with_expiry((chrono::Utc::now() + chrono::TimeDelta::days(30)).timestamp())
        .build();

    let test_container = TestContainer::new(&certs, "test_tls_sha1_intermediate").await;

    let service_def = serde_json::json! {{
        "name": "test",
        "cron_schedule": "0 0 * * *",
        "port": test_container.tls_port,
    }};

    let service: TlsService = serde_json::from_value(service_def).expect("Failed to parse service");
    let bad_hostname = "localhost".to_string();
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
    // TODO: one day work out how to check for a sha1 intermediate
    assert!(result.unwrap().status == ServiceStatus::Ok);
}

#[tokio::test]
async fn test_tls_no_subject() {
    use crate::prelude::*;

    let _ = test_setup().await.expect("Failed to set up test");

    let certs = TestCertificateBuilder::new()
        .without_cert_name()
        .with_expiry((chrono::Utc::now() - chrono::TimeDelta::days(30)).timestamp())
        .with_issue_time((chrono::Utc::now() - chrono::TimeDelta::days(31)).timestamp())
        .build();

    let test_container = TestContainer::new(&certs, "test_tls_no_subject").await;

    let service_def = serde_json::json! {{
        "name": "test",
        "cron_schedule": "0 0 * * *",
        "port": test_container.tls_port,
        "timeout" : 5,
    }};

    let service: TlsService = serde_json::from_value(service_def).expect("Failed to parse service");
    let bad_hostname = "localhost".to_string();
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

// TODO: once we can generate arbitrary test certs, test for one that expires in x days

#[tokio::test]
async fn test_zero_port() {
    let _ = test_setup().await.expect("Failed to set up test");

    let service_def = serde_json::json! {{
        "name": "test",
        "cron_schedule": "0 0 * * *",
        "port": 0,
    }};

    let service: Result<TlsService, serde_json::Error> = serde_json::from_value(service_def);
    assert!(service.is_err());
}

#[tokio::test]
async fn test_timeout() {
    use crate::prelude::*;

    let _ = test_setup().await.expect("Failed to set up test");

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

#[tokio::test]
async fn test_service_parser() {
    let (db, ..) = test_setup().await.expect("Failed to set up test");
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
            port: 1234.try_into().expect("Failed to convert port"),
            expiry_critical: Some(1),
            expiry_warn: Some(7),
            timeout: Some(5),
            jitter: None,
        })),
    };
    let _ = service.parse_config().expect("Failed to parse config!");

    let host = entities::host::Entity::find()
        .one(&*db.write().await)
        .await
        .expect("Failed to search for host")
        .expect("Failed to find host");

    service
        .config
        .unwrap()
        .as_json_pretty(&host)
        .expect("Failed to convert to json");
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
            port: 1234.try_into().expect("Failed to convert port"),
            expiry_critical: Some(1),
            expiry_warn: Some(7),
            timeout: Some(5),
            jitter: None,
        })),
    };
    assert!(service.parse_config().is_err());
}
