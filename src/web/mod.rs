//! Web server related functionality

use std::path::PathBuf;
use std::str::FromStr;

use crate::constants::WEB_SERVER_DEFAULT_STATIC_PATH;
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

use prometheus::Registry;
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tower_sessions::{
    cookie::{time::Duration, SameSite},
    Expiry, MemoryStore, SessionManagerLayer,
};
use views::service_check::service_check_get;

pub(crate) mod oidc;
pub(crate) mod views;

#[derive(Clone)]
pub(crate) struct WebState {
    pub db: Arc<DatabaseConnection>,
    pub frontend_url: Arc<String>,
    pub registry: Option<Arc<Registry>>,
}

impl WebState {
    pub fn new(
        db: Arc<DatabaseConnection>,
        config: &Configuration,
        registry: Option<Arc<Registry>>,
    ) -> Self {
        Self {
            db,
            frontend_url: Arc::new(config.frontend_url()),
            registry,
        }
    }

    #[cfg(test)]
    pub async fn test() -> Self {
        let (db, config) = crate::db::tests::test_setup()
            .await
            .expect("Failed to set up test");
        Self::new(db, &config, None)
    }
    #[cfg(test)]
    pub fn with_registry(self) -> Self {
        let (_provider, registry) =
            crate::metrics::new().expect("Failed to set up metrics provider");
        Self {
            registry: Some(Arc::new(registry)),
            ..self
        }
    }
}

async fn notimplemented(State(_state): State<WebState>) -> Result<(), impl IntoResponse> {
    Err((StatusCode::NOT_FOUND, "Not Implemented yet!"))
}

async fn up(State(_state): State<WebState>) -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

pub(crate) async fn build_app(state: WebState, config: &Configuration) -> Result<Router, Error> {
    let session_store = MemoryStore::default();

    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(Duration::seconds(3600)));

    let mut app = Router::new()
        .route("/auth/login", get(Redirect::temporary("/")))
        .route("/auth/profile", get(views::profile::profile))
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
        .route("/service_check/:service_check_id", get(service_check_get))
        .route("/service/:service_id", get(notimplemented))
        .route("/host_group/:group_id", get(notimplemented))
        .route("/tools", get(views::tools::tools).post(views::tools::tools));
    if config.oidc_enabled {
        app = app.route("/auth/logout", get(oidc::logout)).layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(|e: MiddlewareError| async {
                    e.into_response()
                }))
                .layer(OidcLoginLayer::<EmptyAdditionalClaims>::new()),
        );
    }
    // after here, the routers don't *require* auth

    app = app.route("/", get(views::index::index));
    app = app.route("/metrics", get(views::metrics::metrics));

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
                        vec!["openid", "groups"]
                            .into_iter()
                            .map(|s| s.to_string())
                            .collect(),
                    )
                    .await
                    .map_err(Error::from)?,
                ),
        );
    }
    app = app
        .route("/healthcheck", get(up))
        .nest_service(
            "/static",
            ServeDir::new(
                config
                    .static_path
                    .clone()
                    .unwrap_or(PathBuf::from(WEB_SERVER_DEFAULT_STATIC_PATH)),
            )
            .precompressed_br(),
        )
        .layer(TraceLayer::new_for_http())
        .layer(session_layer);
    // here... we... go!
    Ok(app.with_state(state))
}

#[cfg(not(tarpaulin_include))]
/// Starts up the web server
pub async fn run_web_server(
    configuration: Arc<Configuration>,
    db: Arc<DatabaseConnection>,
    registry: Arc<Registry>,
) -> Result<(), Error> {
    use axum_server::bind_rustls;
    use axum_server::tls_rustls::RustlsConfig;

    let app = build_app(
        WebState::new(db, &configuration, Some(registry)),
        &configuration,
    )
    .await?;

    let frontend_url = configuration.frontend_url();

    info!(
        "Starting web server on {} (listen address is {}",
        &frontend_url,
        configuration.listen_addr()
    );
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    if !configuration.cert_file.exists() {
        return Err(Error::Generic(format!(
            "TLS is enabled but cert_file {:?} does not exist",
            configuration.cert_file
        )));
    }

    if !configuration.cert_key.exists() {
        return Err(Error::Generic(format!(
            "TLS is enabled but cert_key {:?} does not exist",
            configuration.cert_key
        )));
    };
    let tls_config = RustlsConfig::from_pem_file(
        &configuration.cert_file.as_path(),
        &configuration.cert_key.as_path(),
    )
    .await
    .map_err(|err| Error::Generic(format!("Failed to load TLS config: {:?}", err)))?;
    bind_rustls(
        configuration.listen_addr().parse().map_err(|err| {
            Error::Generic(format!(
                "Failed to parse listen address {}: {:?}",
                configuration.listen_addr(),
                err
            ))
        })?,
        tls_config,
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
        let (db, config) = test_setup().await.expect("Failed to set up test");
        let app = build_app(WebState::new(db.clone(), &config, None), &config)
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
            .unwrap_or_else(|err| panic!("Failed to GET {} {:?}", url, err));

        let service_check = entities::service_check::Entity::find()
            .one(db.as_ref())
            .await
            .expect("Failed to query db for service_check")
            .expect("Failed to find service_check");

        let url = format!("/service_check/{}", service_check.id);
        app.oneshot(axum::http::Request::get(&url).body(Body::empty()).unwrap())
            .await
            .unwrap_or_else(|err| panic!("Failed to get {} {:?}", url, err));
    }

    #[tokio::test]
    async fn test_not_implemented() {
        let (db, config) = test_setup().await.expect("Failed to set up test");

        let res = notimplemented(axum::extract::State(WebState::new(db, &config, None))).await;
        assert!(res.is_err());
    }
    #[tokio::test]
    async fn test_up_endpoint() {
        let (db, config) = test_setup().await.expect("Failed to set up test");

        let res = up(axum::extract::State(WebState::new(db, &config, None)))
            .await
            .into_response();
        assert!(res.status() == StatusCode::OK);
    }
}
