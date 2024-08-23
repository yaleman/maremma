use reqwest::StatusCode;

pub(crate) mod host;
pub(crate) mod index;
pub(crate) mod metrics;
pub(crate) mod prelude;
pub(crate) mod profile;
pub(crate) mod service_check;
pub(crate) mod tools;

pub(crate) async fn handler_404() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "nothing to see here")
}
