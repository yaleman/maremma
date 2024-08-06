use crate::prelude::*;

use askama_axum::IntoResponse;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;
use axum_server::bind;

pub(crate) mod views;

#[derive(Clone)]
pub(crate) struct WebState {
    pub db: Arc<DatabaseConnection>,
}

async fn notimplemented(State(_state): State<WebState>) -> Result<(), impl IntoResponse> {
    Err((StatusCode::NOT_FOUND, "Not Implemented yet!"))
}

pub(crate) fn build_app(state: WebState) -> Router {
    Router::new()
        .route("/", get(views::index::index))
        .route("/host/:host_id", get(views::host::host))
        .route("/service_check/:service_check_id", get(notimplemented))
        .route(
            "/service_check/:service_check_id/set_urgent",
            post(notimplemented),
        )
        .route(
            "/service_check/:service_check_id/disable",
            post(notimplemented),
        )
        .route("/service/:service_id", get(notimplemented))
        .route("/host_group/:group_id", get(notimplemented))
        .with_state(state)
}

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

    // use crate::prelude::*;
    // use crate::setup_logging;
    // use crate::web::run_web_server;

    // TODO: work out how to test the startup of the server
    // #[tokio::test]
    // async fn test_run_web_server() {
    //     let _ = setup_logging(true);

    //     let mut configuration = Configuration::load_test_config().await;
    //     configuration.listen_port = Some(rand::random::<u16>());

    //     let mut attempts = 0;
    //     loop {
    //         // don't test on the standard port
    //         if configuration.listen_port == Some(8888) {
    //             continue;
    //         }
    //         // don't let it run on low ports
    //         if let Some(port) = configuration.listen_port {
    //             if port < 4096 {
    //                 continue;
    //             }
    //         }
    //         // test to see if we can connect to the port
    //         if let Ok(listener) = std::net::TcpListener::bind(format!(
    //             "{}:{}",
    //             configuration.listen_address,
    //             configuration.listen_port.unwrap()
    //         )) {
    //             drop(listener);
    //             break;
    //         }
    //         configuration.listen_port = Some(rand::random::<u16>());
    //         attempts += 1;
    //         if attempts > 5 {
    //             panic!("Failed to find a port to bind to");
    //         }
    //     }
    //     debug!("Using port: {:?}", configuration.listen_port);
    //     let db = Arc::new(crate::db::test_connect().await.unwrap());
    //     let _result = tokio::spawn(run_web_server(Arc::new(configuration), db));

    //     let _ = tokio::time::sleep(tokio::time::Duration::from_micros(500)).await;
    //     drop(_result);
    // }
}
