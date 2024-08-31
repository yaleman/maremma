//! Web server related functionality
//!

pub(crate) mod controller;
pub(crate) mod oidc;
pub(crate) mod views;
use controller::WebServerControl;
use tokio::sync::RwLockReadGuard;

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
use axum_server::bind_rustls;
use axum_server::tls_rustls::RustlsConfig;
use tokio::sync::mpsc::{Receiver, Sender};

use prometheus::Registry;
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tower_sessions::{
    cookie::{time::Duration, SameSite},
    Expiry, SessionManagerLayer,
};
use views::handler_404;
use views::host_group::{host_group, host_group_delete, host_group_member_delete, host_groups};
use views::service_check::{service_check_delete, service_check_get};

#[derive(Clone)]
pub(crate) struct WebState {
    pub db: Arc<DatabaseConnection>,
    pub configuration: SendableConfig,
    pub registry: Option<Arc<Registry>>,
    pub web_tx: Option<Sender<WebServerControl>>,
    pub config_filepath: PathBuf,
}

impl WebState {
    pub fn new(
        db: Arc<DatabaseConnection>,
        configuration: SendableConfig,
        registry: Option<Arc<Registry>>,
        web_tx: Option<Sender<WebServerControl>>,
        config_filepath: PathBuf,
    ) -> Self {
        Self {
            db,
            configuration,
            registry,
            web_tx,
            config_filepath,
        }
    }

    #[cfg(test)]
    pub async fn test() -> Self {
        let (db, config) = crate::db::tests::test_setup()
            .await
            .expect("Failed to set up test");
        Self::new(db, config, None, None, PathBuf::new())
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

/// Create the database-backed session store
pub fn get_session_store(db: &Arc<DatabaseConnection>) -> entities::session::ModelStore {
    crate::db::entities::session::ModelStore::new(db.clone())
}

#[derive(Clone)]
struct OidcErrorHandler {
    web_tx: Option<Sender<WebServerControl>>,
}

impl OidcErrorHandler {
    pub fn new(web_tx: Option<Sender<WebServerControl>>) -> Self {
        Self { web_tx }
    }

    async fn handle_oidc_error(&self) {
        if let Some(tx) = &self.web_tx {
            let _ = tx.send(WebServerControl::Stop).await;
        }
    }
}

#[cfg(not(tarpaulin_include))]
pub(crate) async fn build_app(state: WebState) -> Result<Router, Error> {
    // get all the config variables we need, quickly, so we can drop the lock
    let config_reader = state.configuration.read().await;
    let oidc_issuer = config_reader.oidc_issuer.clone();
    let oidc_client_id = config_reader.oidc_client_id.clone();
    let oidc_client_secret = config_reader.oidc_client_secret.clone();
    let frontend_url = config_reader.frontend_url.clone();
    drop(config_reader);

    let session_store = get_session_store(&state.db);

    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(true)
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(Duration::seconds(1800)));

    let frontend_url = Uri::from_str(&frontend_url)
        .map_err(|err| Error::Configuration(format!("Failed to parse base_url: {:?}", err)))?;
    debug!("Frontend URL: {:?}", frontend_url);
    let oidc_error_handler = OidcErrorHandler::new(state.web_tx.clone());

    let oidc_login_service = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|e: MiddlewareError| async {
            error!("Failed to handle OIDC logout: {:?}", e);
            e.into_response()
        }))
        .layer(OidcLoginLayer::<EmptyAdditionalClaims>::new());

    let oidc_auth_layer = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|e: MiddlewareError| async move {
            // TODO: cause this to make the web server restart if it fails
            oidc_error_handler.handle_oidc_error().await;
            error!("Failed to handle OIDC in middleware: {:?}", &e);
            Redirect::to("/auth/logout").into_response()
        }))
        .layer(
            OidcAuthLayer::<EmptyAdditionalClaims>::discover_client(
                frontend_url,
                oidc_issuer,
                oidc_client_id,
                oidc_client_secret,
                vec!["openid", "groups"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect(),
            )
            .await
            .map_err(|err| {
                error!("Failed to set up OIDC: {:?}", err);
                Error::from(err)
            })?,
        );

    let app = Router::new()
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
        .route(
            "/service_check/:service_check_id/delete",
            post(service_check_delete),
        )
        .route("/service_check/:service_check_id", get(service_check_get))
        .route("/hosts", get(views::host::hosts))
        .route("/host/:host_id", get(views::host::host))
        .route("/host/:host_id/delete", post(views::host::delete_host))
        .route("/service/:service_id", get(notimplemented))
        .route("/host_group/:group_id", get(host_group))
        .route("/host_group/:group_id/delete", post(host_group_delete))
        .route(
            "/host_group/:group_id/member/:host_id/delete",
            post(host_group_member_delete),
        )
        .route("/host_groups", get(host_groups))
        .route("/tools", get(views::tools::tools).post(views::tools::tools))
        .route("/auth/logout", get(oidc::logout))
        .route("/auth/rp-logout", get(oidc::rp_logout))
        .layer(oidc_login_service)
        // after here, the routers don't *require* auth
        .route("/", get(views::index::index))
        .route("/metrics", get(views::metrics::metrics))
        .layer(oidc_auth_layer)
        // after here, the URLs cannot have auth
        .route("/healthcheck", get(up))
        .nest_service(
            "/static",
            ServeDir::new(
                state
                    .configuration
                    .read()
                    .await
                    .static_path
                    .clone()
                    .unwrap_or(PathBuf::from(WEB_SERVER_DEFAULT_STATIC_PATH)),
            )
            .precompressed_br(),
        )
        .fallback(handler_404)
        .layer(TraceLayer::new_for_http())
        .layer(session_layer);
    // here... we... go!
    Ok(app.with_state(state))
}

fn check_certs_exist(
    config_reader: &RwLockReadGuard<'_, Configuration>,
) -> Result<(PathBuf, PathBuf), Error> {
    let cert_file = config_reader.cert_file.clone();
    let cert_key = config_reader.cert_key.clone();
    if !cert_file.exists() {
        return Err(Error::Generic(format!(
            "TLS is enabled but cert_file {:?} does not exist",
            cert_file
        )));
    }

    if !cert_key.exists() {
        return Err(Error::Generic(format!(
            "TLS is enabled but cert_key {:?} does not exist",
            cert_key
        )));
    };
    Ok((cert_file, cert_key))
}

/// Start and run the web server
#[cfg(not(tarpaulin_include))]
pub async fn start_web_server(configuration: SendableConfig, app: Router) -> Result<(), Error> {
    let configuration_reader = configuration.read().await;

    let listen_address = configuration_reader.listen_addr();
    let (cert_file, cert_key) = check_certs_exist(&configuration_reader)?;
    drop(configuration_reader);

    let tls_config = RustlsConfig::from_pem_file(&cert_file.as_path(), &cert_key.as_path())
        .await
        .map_err(|err| Error::Generic(format!("Failed to load TLS config: {:?}", err)))?;
    bind_rustls(
        listen_address.parse().map_err(|err| {
            Error::Generic(format!(
                "Failed to parse listen address {}: {:?}",
                listen_address, err
            ))
        })?,
        tls_config,
    )
    .serve(app.into_make_service())
    .await
    .map_err(|err| Error::Generic(format!("Web server failed: {:?}", err)))
}

#[cfg(not(tarpaulin_include))]
/// Starts up the web server
pub async fn run_web_server(
    config_filepath: PathBuf,
    configuration: SendableConfig,
    db: Arc<DatabaseConnection>,
    registry: Arc<Registry>,
    web_tx: Sender<WebServerControl>,
    mut web_server_controller: Receiver<WebServerControl>,
) -> Result<(), Error> {
    let app = build_app(
        // TODO web_tx impl
        WebState::new(
            db,
            configuration.clone(),
            Some(registry),
            Some(web_tx),
            config_filepath,
        ),
    )
    .await?;

    let frontend_url = configuration.read().await.frontend_url.clone();

    info!(
        "Starting web server on {} (listen address is {}",
        &frontend_url,
        configuration.read().await.listen_addr()
    );
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    loop {
        tokio::select! {
            server_result = start_web_server(configuration.clone(), app.clone()) => {
                match server_result {Ok(_) => {
                    error!("Web server exited cleanly");
                },
                Err(err) => {
                    error!("Web server failed: {:?}", err);
                    return Err(err)
                }}
            },
            server_message = web_server_controller.recv() => {
                match server_message {
                    Some(WebServerControl::Stop) => {
                        info!("Web server stopping");
                        return Ok(());
                    },
                    Some(WebServerControl::Reload) => {
                        info!("Web server reloading");
                    },
                    None => {
                        error!("Web server controller channel closed");
                        return Ok(())
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::db::tests::test_setup;
    use crate::tests::tls_utils::TestCertificateBuilder;
    use axum::body::Body;
    use entities::host;
    // TODO: work out how to test the startup of the server
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn test_app_requests() {
        if std::env::var("CI").is_ok() {
            eprintln!("Skipping test because it fails in CI");
            return;
        }
        let (db, config) = test_setup().await.expect("Failed to set up test");
        let app = build_app(WebState::new(
            db.clone(),
            config.clone(),
            None,
            None,
            PathBuf::new(),
        ))
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

        let res = notimplemented(axum::extract::State(WebState::new(
            db,
            config.clone(),
            None,
            None,
            PathBuf::new(),
        )))
        .await;
        assert!(res.is_err());
    }
    #[tokio::test]
    async fn test_up_endpoint() {
        let (db, config) = test_setup().await.expect("Failed to set up test");

        let res = up(axum::extract::State(WebState::new(
            db,
            config.clone(),
            None,
            None,
            PathBuf::new(),
        )))
        .await
        .into_response();
        assert!(res.status() == StatusCode::OK);
    }

    #[tokio::test]
    async fn test_oidcerrorhandler() {
        let _ = test_setup().await.expect("Failed to set up test");

        let _res = OidcErrorHandler::new(None).handle_oidc_error().await;

        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        let _res = OidcErrorHandler::new(Some(tx)).handle_oidc_error().await;
    }

    #[tokio::test]
    async fn test_check_certs_exist() {
        let (_db, config) = test_setup().await.expect("Failed to set up test");

        let certs = TestCertificateBuilder::new()
            .with_name("localhost")
            .with_expiry((chrono::Utc::now() + chrono::TimeDelta::days(30)).timestamp())
            .with_issue_time((chrono::Utc::now() - chrono::TimeDelta::days(30)).timestamp())
            .build();

        let mut config_writer = config.write().await;
        config_writer.cert_file = certs.cert_file.path().to_path_buf();
        config_writer.cert_key = certs.cert_file.path().to_path_buf();
        drop(config_writer);

        let (_cert_file, _cert_key) =
            check_certs_exist(&config.read().await).expect("Failed to check certs");

        let mut config_writer = config.write().await;
        config_writer.cert_file = PathBuf::from("/asdfasdf");
        drop(config_writer);

        assert!(check_certs_exist(&config.read().await).is_err());
        let mut config_writer = config.write().await;
        config_writer.cert_file = certs.cert_file.path().to_path_buf();
        config_writer.cert_key = PathBuf::from("/asdfasdf");
        drop(config_writer);

        assert!(check_certs_exist(&config.read().await).is_err());
    }
}
