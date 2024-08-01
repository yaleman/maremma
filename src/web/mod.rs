use crate::prelude::*;

use axum::routing::get;
use axum::Router;
use axum_server::bind;

pub(crate) mod views;

#[derive(Clone)]
pub(crate) struct WebState {
    pub configuration: Arc<Configuration>,
}

pub async fn run_web_server(configuration: Arc<Configuration>) -> Result<(), Error> {
    let app = Router::new()
        .route("/", get(views::index::index))
        .route("/host/:host_id", get(views::host::host))
        .with_state(WebState { configuration });
    let addr = "127.0.0.1:8888";

    info!("Starting web server on http://{}", &addr);
    bind(
        addr.parse()
            .map_err(|err| Error::Generic(format!("Failed to parse address: {err:?}")))?,
    )
    .serve(app.into_make_service())
    .await
    .map_err(|err| Error::Generic(format!("Web server failed: {:?}", err)))
}
