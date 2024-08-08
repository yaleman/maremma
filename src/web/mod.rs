use std::path::PathBuf;
use std::str::FromStr;

use crate::prelude::*;

use askama_axum::IntoResponse;
use axum::error_handling::HandleErrorLayer;
use axum::extract::State;
use axum::http::{StatusCode, Uri};
use axum::response::Redirect;
use axum::routing::{get, post};
use axum::Router;
use axum_oidc::error::MiddlewareError;
use axum_oidc::{EmptyAdditionalClaims, OidcAuthLayer, OidcLoginLayer};

use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tower_sessions::{
    cookie::{time::Duration, SameSite},
    Expiry, MemoryStore, SessionManagerLayer,
};

pub(crate) mod oidc;
pub(crate) mod views;

#[derive(Clone)]
pub(crate) struct WebState {
    pub db: Arc<DatabaseConnection>,
    pub frontend_url: Arc<String>,
}

impl WebState {
    pub fn new(db: Arc<DatabaseConnection>, config: &Configuration) -> Self {
        Self {
            db,
            frontend_url: Arc::new(config.frontend_url()),
        }
    }
}

async fn notimplemented(State(_state): State<WebState>) -> Result<(), impl IntoResponse> {
    Err((StatusCode::NOT_FOUND, "Not Implemented yet!"))
}

pub(crate) async fn build_app(state: WebState, config: &Configuration) -> Result<Router, Error> {
    let session_store = MemoryStore::default();

    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(Duration::seconds(3600)));

    let static_path = PathBuf::from("./static");

    let mut app = Router::new()
        .route("/auth/login", get(Redirect::temporary("/")))
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
        .route("/host/:host_id", get(views::host::host))
        .route("/service_check/:service_check_id", get(notimplemented))
        .route("/service/:service_id", get(notimplemented))
        .route("/host_group/:group_id", get(notimplemented));

    if config.oidc_enabled {
        app = app.route("/auth/logout", get(oidc::logout)).layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(|e: MiddlewareError| async {
                    e.into_response()
                }))
                .layer(OidcLoginLayer::<EmptyAdditionalClaims>::new()),
        );
    }

    app = app.route("/", get(views::index::index));

    if config.oidc_enabled {
        let (issuer, client_id, client_secret) = if let Some(oidc_config) = &config.oidc_config {
            (
                oidc_config.issuer.clone(),
                oidc_config.client_id.clone(),
                oidc_config.client_secret.clone(),
            )
        } else {
            return Err(Error::Generic(
                "OIDC is enabled but no OIDC config is provided".to_string(),
            ));
        };

        let frontend_url = config
            .frontend_url
            .clone()
            .ok_or_else(|| Error::Generic("Frontend URL is required for OIDC".to_string()))?;

        app = app.layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(|e: MiddlewareError| async {
                    e.into_response()
                }))
                .layer(
                    OidcAuthLayer::<EmptyAdditionalClaims>::discover_client(
                        Uri::from_str(&frontend_url).map_err(|err| {
                            Error::Generic(format!("Failed to parse base_url: {:?}", err))
                        })?,
                        issuer,
                        client_id,
                        client_secret,
                        vec![],
                    )
                    .await
                    .map_err(Error::from)?,
                ),
        );
    }
    app = app
        .nest_service("/static", ServeDir::new(static_path).precompressed_br())
        .layer(TraceLayer::new_for_http())
        .layer(session_layer);
    // here... we... go!
    Ok(app.with_state(state))
}

#[cfg(not(tarpaulin_include))] // TODO: tarpaulin un-ignore for code coverage
pub async fn run_web_server(
    configuration: Arc<Configuration>,
    db: Arc<DatabaseConnection>,
) -> Result<(), Error> {
    use axum_server::bind_rustls;
    use axum_server::tls_rustls::RustlsConfig;

    let addr = format!(
        "{}:{}",
        configuration.listen_address,
        configuration
            .listen_port
            .unwrap_or(crate::constants::DEFAULT_PORT)
    );
    let app = build_app(WebState::new(db, &configuration), &configuration).await?;

    let frontend_url = configuration.frontend_url();

    info!("Starting web server on {}", &frontend_url);
    if configuration.tls_enabled {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        let cert_file = match &configuration.cert_file {
            Some(cert_file) => {
                if !cert_file.exists() {
                    return Err(Error::Generic(format!(
                        "TLS is enabled but cert_file {:?} does not exist",
                        cert_file
                    )));
                }
                cert_file
            }
            None => {
                return Err(Error::Generic(
                    "TLS is enabled but no cert_file is provided".to_string(),
                ))
            }
        };
        let cert_key = match &configuration.cert_key {
            Some(cert_key) => {
                if !cert_key.exists() {
                    return Err(Error::Generic(format!(
                        "TLS is enabled but cert_key {:?} does not exist",
                        cert_key
                    )));
                }
                cert_key
            }
            None => {
                return Err(Error::Generic(
                    "TLS is enabled but no cert_key is provided".to_string(),
                ))
            }
        };
        let tls_config = RustlsConfig::from_pem_file(&cert_file.as_path(), &cert_key.as_path())
            .await
            .map_err(|err| Error::Generic(format!("Failed to load TLS config: {:?}", err)))?;
        bind_rustls(
            addr.parse()
                .map_err(|err| Error::Generic(format!("Failed to parse address: {err:?}")))?,
            tls_config,
        )
        .serve(app.into_make_service())
        .await
        .map_err(|err| Error::Generic(format!("Web server failed: {:?}", err)))
    } else {
        axum_server::bind(
            addr.parse()
                .map_err(|err| Error::Generic(format!("{:?}", err)))?,
        )
        .serve(app.into_make_service())
        .await
        .map_err(|err| Error::Generic(format!("Web server failed: {:?}", err)))
    }
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
        let (db, config) = test_setup().await.expect("Failed to set up test");
        let app = build_app(WebState::new(db.clone(), &config), &config)
            .await
            .expect("Failed to build app");

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
        let (db, config) = test_setup().await.expect("Failed to set up test");

        let res = notimplemented(axum::extract::State(WebState::new(db, &config))).await;
        assert!(res.is_err());
    }
}
