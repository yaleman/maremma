use std::process::Command;

#[test]
fn valid_configuration_does_not_create_database() {
    let tempdir = tempfile::tempdir().expect("Failed to create temporary directory");
    let config_path = tempdir.path().join("maremma.json");
    let database_path = tempdir.path().join("must-not-exist.sqlite");
    let config = serde_json::json!({
        "database_file": database_path,
        "hosts": {},
        "services": {},
        "frontend_url": "https://maremma.example.test",
        "oidc_issuer": "https://idm.example.test/oauth2/openid/maremma",
        "oidc_client_id": "maremma",
        "cert_file": "",
        "cert_key": "",
        "max_history_entries_per_check": 10
    });
    std::fs::write(
        &config_path,
        serde_json::to_vec(&config).expect("Failed to serialize configuration"),
    )
    .expect("Failed to write configuration");

    let result = Command::new(env!("CARGO_BIN_EXE_maremma"))
        .args(["check-config", "--config"])
        .arg(&config_path)
        .status()
        .expect("Failed to run maremma check-config");

    assert!(result.success());
    assert!(!database_path.exists());
}

#[test]
fn invalid_configuration_returns_failure_without_creating_database() {
    let tempdir = tempfile::tempdir().expect("Failed to create temporary directory");
    let config_path = tempdir.path().join("maremma.json");
    let database_path = tempdir.path().join("must-not-exist.sqlite");
    let config = serde_json::json!({
        "database_file": database_path,
        "hosts": {},
        "services": {
            "invalid": {
                "service_type": "not-a-service",
                "host_groups": [],
                "cron_schedule": "@hourly"
            }
        },
        "frontend_url": "https://maremma.example.test",
        "oidc_issuer": "https://idm.example.test/oauth2/openid/maremma",
        "oidc_client_id": "maremma",
        "cert_file": "",
        "cert_key": "",
        "max_history_entries_per_check": 10
    });
    std::fs::write(
        &config_path,
        serde_json::to_vec(&config).expect("Failed to serialize configuration"),
    )
    .expect("Failed to write configuration");

    let result = Command::new(env!("CARGO_BIN_EXE_maremma"))
        .args(["check-config", "--config"])
        .arg(&config_path)
        .status()
        .expect("Failed to run maremma check-config");

    assert!(!result.success());
    assert!(!database_path.exists());
}
