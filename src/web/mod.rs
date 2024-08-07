use std::path::PathBuf;

use crate::prelude::*;

use askama_axum::IntoResponse;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;
use axum_server::bind;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

pub(crate) mod views;

#[derive(Clone)]
pub(crate) struct WebState {
    pub db: Arc<DatabaseConnection>,
}

#[cfg(test)]
impl WebState {
    pub(crate) fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }
}

async fn notimplemented(State(_state): State<WebState>) -> Result<(), impl IntoResponse> {
    Err((StatusCode::NOT_FOUND, "Not Implemented yet!"))
}

pub(crate) fn build_app(state: WebState) -> Router {
    let static_path = PathBuf::from("./static");

    Router::new()
        .route("/", get(views::index::index))
        .route("/host/:host_id", get(views::host::host))
        .route("/service_check/:service_check_id", get(notimplemented))
        .route(
            "/service_check/:service_check_id/urgent",
            post(views::service_check::set_service_check_urgent),
        )
        .route(
            "/service_check/:service_check_id/disable",
            post(views::service_check::set_service_check_disabled),
        )
        .route(
            "/service_check/:service_check_id/enable",
            post(views::service_check::set_service_check_enabled),
        )
        .route("/service/:service_id", get(notimplemented))
        .route("/host_group/:group_id", get(notimplemented))
        .nest_service("/static", ServeDir::new(static_path).precompressed_br())
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

#[cfg(not(tarpaulin_include))] // TODO: tarpaulin un-ignore for code coverage
pub async fn run_web_server(
    configuration: Arc<Configuration>,
    db: Arc<DatabaseConnection>,
) -> Result<(), Error> {
    let addr = format!(
        "{}:{}",
        configuration.listen_address,
        configuration.listen_port.unwrap_or(8888)
    );
    let app = build_app(WebState { db });

    info!("Starting web server on http://{}", &addr);
    bind(
        addr.parse()
            .map_err(|err| Error::Generic(format!("Failed to parse address: {err:?}")))?,
    )
    .serve(app.into_make_service())
    .await
    .map_err(|err| Error::Generic(format!("Web server failed: {:?}", err)))
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::db::tests::test_setup;
    use axum::body::Body;
    use entities::host;
    // TODO: work out how to test the startup of the server
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn test_app_requests() {
        let (db, _config) = test_setup().await.expect("Failed to set up test");
        let app = build_app(WebState::new(db.clone()));

        app.clone()
            .oneshot(axum::http::Request::get("/").body(Body::empty()).unwrap())
            .await
            .expect("Failed to run app");

        let host = host::Entity::find()
            .one(db.as_ref())
            .await
            .expect("Failed to query db for host")
            .expect("Failed to find host");

        let url = format!("/host/{}", host.id);
        app.clone()
            .oneshot(axum::http::Request::get(&url).body(Body::empty()).unwrap())
            .await
            .expect(&format!("Failed to GET {}", url));

        let service_check = entities::service_check::Entity::find()
            .one(db.as_ref())
            .await
            .expect("Failed to query db for service_check")
            .expect("Failed to find service_check");

        let url = format!("/service_check/{}", service_check.id);
        app.oneshot(axum::http::Request::get(&url).body(Body::empty()).unwrap())
            .await
            .expect(&format!("Failed to get {}", url));
    }

    #[tokio::test]
    async fn test_not_implemented() {
        let (db, _config) = test_setup().await.expect("Failed to set up test");

        let res = notimplemented(axum::extract::State(WebState::new(db))).await;
        assert!(res.is_err());
    }
}
