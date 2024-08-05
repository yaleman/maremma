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

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::setup_logging;
    use crate::web::run_web_server;

    #[tokio::test]
    async fn test_run_web_server() {
        let _ = setup_logging(true);

        let mut configuration = Configuration::load_test_config().await;
        configuration.listen_port = Some(rand::random::<u16>());
        loop {
            // don't test on the standard port
            if configuration.listen_port == Some(8888) {
                continue;
            }
            // don't let it run on low ports
            if let Some(port) = configuration.listen_port {
                if port < 4096 {
                    continue;
                }
            }
            // test to see if we can connect to the port
            if let Ok(listener) = std::net::TcpListener::bind(format!(
                "{}:{}",
                configuration.listen_address,
                configuration.listen_port.unwrap()
            )) {
                drop(listener);
                break;
            }
            configuration.listen_port = Some(rand::random::<u16>());
        }
        debug!("Using port: {:?}", configuration.listen_port);
        let db = Arc::new(crate::db::test_connect().await.unwrap());
        let _result = tokio::spawn(run_web_server(Arc::new(configuration), db));

        let _ = tokio::time::sleep(tokio::time::Duration::from_micros(500)).await;
    }
}
