pub(crate) mod tls_utils;

#[test]
fn test_default_config_file() {
    assert_eq!(crate::DEFAULT_CONFIG_FILE, "maremma.json");
}
