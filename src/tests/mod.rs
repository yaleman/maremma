#[cfg(test)]
pub(crate) mod tls_utils;

pub(crate) mod testcontainers;

#[test]
fn test_default_config_file() {
    assert_eq!(crate::DEFAULT_CONFIG_FILE, "maremma.json");
}
