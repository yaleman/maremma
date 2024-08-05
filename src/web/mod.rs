use crate::prelude::*;

use axum::routing::get;
use axum::Router;
use axum_server::bind;

pub(crate) mod views;

#[derive(Clone)]
pub(crate) struct WebState {
    pub db: Arc<DatabaseConnection>,
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
    let app = Router::new()
        .route("/", get(views::index::index))
        .route("/host/:host_id", get(views::host::host))
        .with_state(WebState { db });

    info!("Starting web server on http://{}", &addr);
    bind(
        addr.parse()
            .map_err(|err| Error::Generic(format!("Failed to parse address: {err:?}")))?,
    )
    .serve(app.into_make_service())
    .await
    .map_err(|err| Error::Generic(format!("Web server failed: {:?}", err)))
}
