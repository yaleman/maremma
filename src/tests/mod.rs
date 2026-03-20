#[cfg(test)]
pub(crate) mod tls_utils;

pub(crate) mod testcontainers;

#[cfg(test)]
const LIVE_TEST_ENV_VAR: &str = "MAREMMA_RUN_LIVE_TESTS";

#[cfg(test)]
fn live_tests_requested() -> bool {
    std::env::var(LIVE_TEST_ENV_VAR)
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes"
        })
        .unwrap_or(false)
}

#[cfg(test)]
/// Returns whether the current test should run live network or Docker-backed checks.
pub(crate) fn require_live_tests(test_name: &str) -> bool {
    if live_tests_requested() {
        true
    } else {
        eprintln!(
            "Skipping {test_name}; set {LIVE_TEST_ENV_VAR}=1 to enable live network/Docker tests"
        );
        false
    }
}

#[test]
fn test_default_config_file() {
    assert_eq!(crate::DEFAULT_CONFIG_FILE, "maremma.json");
}
