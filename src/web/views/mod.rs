use axum::http::StatusCode;

pub(crate) mod host;
pub(crate) mod host_group;
pub(crate) mod index;
pub(crate) mod metrics;
pub(crate) mod prelude;
pub(crate) mod profile;
pub(crate) mod service_check;
pub(crate) mod tools;

pub(crate) async fn handler_404() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "nothing to see here")
}

#[tokio::test]
async fn test_handler_404() {
    // This is a dummy test to ensure that the views compile
    // and that the test functions are present.
    // The actual tests are in the individual view modules.
    assert_eq!(
        handler_404().await,
        (StatusCode::NOT_FOUND, "nothing to see here")
    );
}
