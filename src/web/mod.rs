//! Web server related functionality
//!

pub(crate) mod controller;
pub(crate) mod oidc;
pub(crate) mod urls;
pub(crate) mod views;
#[cfg(test)]
use tempfile::NamedTempFile;

use std::path::PathBuf;
use std::str::FromStr;

use axum::error_handling::HandleErrorLayer;
use axum::extract::State;
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{any, get, post};
use axum::Router;
use axum_oidc::error::MiddlewareError;
use axum_oidc::{EmptyAdditionalClaims, OidcAuthLayer, OidcLoginLayer};
use axum_server::bind_rustls;
use axum_server::tls_rustls::RustlsConfig;
use prometheus::Registry;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::RwLockReadGuard;
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tower_sessions::{
    cookie::{time::Duration, SameSite},
    Expiry, SessionManagerLayer,
};

#[cfg(test)]
use axum::extract::Request;
#[cfg(test)]
use axum::middleware::{from_fn, Next};

use crate::constants::WEB_SERVER_DEFAULT_STATIC_PATH;
use crate::prelude::*;
use controller::WebServerControl;
use urls::Urls;
use views::handler_404;
use views::host_group::{host_group, host_group_delete, host_group_member_delete, host_groups};
use views::service::service;
use views::service_check::{service_check_delete, service_check_get};

#[derive(Clone)]
pub(crate) struct WebState {
    db: Arc<DatabaseConnection>,
    pub configuration: SendableConfig,
    registry: Option<Arc<Registry>>,
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

    pub fn db(&self) -> &DatabaseConnection {
        self.db.as_ref()
    }

    #[cfg(test)]
    pub async fn test() -> Self {
        let (db, config) = crate::db::tests::test_setup()
            .await
            .expect("Failed to set up test");
        Self::new(db, config, None, None, PathBuf::new())
    }

    #[cfg(test)]
    /// for when you need a real database for a bit, used in the export DB test for example
    pub async fn test_with_real_db() -> (NamedTempFile, Self) {
        let (tempfile, db, config) = crate::db::tests::test_setup_with_real_db()
            .await
            .expect("Failed to set up test");
        (tempfile, Self::new(db, config, None, None, PathBuf::new()))
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

    #[cfg(test)]
    pub fn get_session(&self) -> tower_sessions::Session {
        let session_store = get_session_store(&self.db);
        tower_sessions::Session::new(None, std::sync::Arc::new(session_store), None)
    }

    pub fn new_csrf_token(&self) -> String {
        rand::random::<u64>().to_string()
    }
}

// async fn notimplemented(State(_state): State<WebState>) -> Result<(), impl IntoResponse> {
//     Err((StatusCode::NOT_FOUND, "Not Implemented yet!"))
// }

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

const RELOAD_TIME: u64 = 1000;
#[cfg(test)]
const TEST_OIDC_SESSION_KEY: &str = "maremma-test-oidc-authenticated";

impl OidcErrorHandler {
    pub fn new(web_tx: Option<Sender<WebServerControl>>) -> Self {
        Self { web_tx }
    }

    async fn handle_oidc_error(&self, error: &MiddlewareError) {
        if let Some(tx) = &self.web_tx {
            error!(
                "Reloading web server in {}ms due to OIDC error: {:?}",
                RELOAD_TIME, error
            );
            let _ = tx.send(WebServerControl::ReloadAfter(RELOAD_TIME)).await;
        }
    }
}

#[cfg(not(tarpaulin_include))]
pub(crate) async fn build_app(state: WebState) -> Result<Router, Error> {
    build_app_inner(state, true).await
}

async fn build_app_inner(state: WebState, enable_oidc: bool) -> Result<Router, Error> {
    let static_path = state
        .configuration
        .read()
        .await
        .static_path
        .clone()
        .unwrap_or(PathBuf::from(WEB_SERVER_DEFAULT_STATIC_PATH));

    let session_store = get_session_store(&state.db);

    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(true)
        .with_same_site(SameSite::Lax)
        .with_http_only(true)
        .with_expiry(Expiry::OnInactivity(Duration::seconds(1800)));

    let protected_routes = Router::new()
        .route(Urls::Profile.as_ref(), get(views::profile::profile))
        .route(Urls::Services.as_ref(), get(views::service::services))
        .route(
            &format!("{}/{{service_check_id}}/urgent", Urls::ServiceCheck),
            post(views::service_check::set_service_check_urgent),
        )
        .route(
            &format!("{}/{{service_check_id}}/disable", Urls::ServiceCheck),
            post(views::service_check::set_service_check_disabled),
        )
        .route(
            &format!("{}/{{service_check_id}}/enable", Urls::ServiceCheck),
            post(views::service_check::set_service_check_enabled),
        )
        .route(
            &format!("{}/{{service_check_id}}/delete", Urls::ServiceCheck),
            post(service_check_delete),
        )
        .route(
            &format!("{}/{{service_check_id}}", Urls::ServiceCheck),
            get(service_check_get),
        )
        .route(Urls::Hosts.as_ref(), get(views::host::hosts))
        .route(
            &format!("{}/{{host_id}}", Urls::Host),
            get(views::host::host),
        )
        .route(
            &format!("{}/{{host_id}}/delete", Urls::Host),
            post(views::host::delete_host),
        )
        .route(&format!("{}/{{service_id}}", Urls::Service), get(service))
        .route(
            &format!("{}/{{group_id}}", Urls::HostGroup),
            get(host_group),
        )
        .route(
            &format!("{}/{{group_id}}/delete", Urls::HostGroup),
            post(host_group_delete),
        )
        .route(
            &format!("{}/{{group_id}}/member/{{host_id}}/delete", Urls::HostGroup),
            post(host_group_member_delete),
        )
        .route(Urls::HostGroups.as_ref(), get(host_groups))
        .route(
            Urls::Tools.as_ref(),
            get(views::tools::tools).post(views::tools::tools),
        )
        .route(Urls::ToolsExportDb.as_ref(), post(views::tools::export_db))
        .route(Urls::RpLogout.as_ref(), get(oidc::rp_logout));
    let auth_only_routes = Router::new().route(Urls::Index.as_ref(), get(views::index::index));
    let public_routes = Router::new()
        .route(Urls::Metrics.as_ref(), get(views::metrics::metrics))
        .route(Urls::HealthCheck.as_ref(), get(up))
        .route(Urls::Logout.as_ref(), get(oidc::logout))
        .nest_service(
            Urls::Static.as_ref(),
            ServeDir::new(static_path).precompressed_br(),
        )
        .fallback(handler_404);

    let app = if enable_oidc {
        use axum_oidc::OidcClient;

        let config_reader = state.configuration.read().await;
        let oidc_issuer = config_reader.oidc_issuer.clone();
        let oidc_client_id = config_reader.oidc_client_id.clone();
        let oidc_client_secret = config_reader.oidc_client_secret.clone();
        let frontend_url = config_reader.frontend_url.clone();
        drop(config_reader);

        let oidc_redirect_url = oidc_redirect_uri(&frontend_url)?;
        debug!("Frontend URL: {:?}", frontend_url);
        debug!("OIDC redirect URL: {:?}", oidc_redirect_url);
        let oidc_error_handler = OidcErrorHandler::new(state.web_tx.clone());
        let oidc_callback_routes = Router::new().route(
            Urls::Login.as_ref(),
            any(axum_oidc::handle_oidc_redirect::<EmptyAdditionalClaims>),
        );

        let oidc_login_service = ServiceBuilder::new()
            .layer(HandleErrorLayer::new(|e: MiddlewareError| async {
                error!("Failed to handle OIDC logout: {:?}", e);
                e.into_response()
            }))
            .layer(OidcLoginLayer::<EmptyAdditionalClaims>::new());

        let mut oidc_client = OidcClient::builder()
            .with_default_http_client()
            .add_scope("openid")
            .add_scope("groups")
            .with_redirect_url(oidc_redirect_url)
            .with_client_id(oidc_client_id);

        if let Some(oidc_client_secret) = oidc_client_secret {
            oidc_client = oidc_client.with_client_secret(oidc_client_secret);
        }

        let oidc_client: OidcClient<EmptyAdditionalClaims> =
            oidc_client.discover(oidc_issuer).await?.build();

        let oidc_auth_layer: OidcAuthLayer<EmptyAdditionalClaims> =
            OidcAuthLayer::<EmptyAdditionalClaims>::new(oidc_client);

        let oidc_auth_service = ServiceBuilder::new()
            .layer(HandleErrorLayer::new(|e: MiddlewareError| async move {
                if let MiddlewareError::SessionNotFound = e {
                    error!("No OIDC session found, redirecting to logout to clear it client-side");
                } else {
                    oidc_error_handler.handle_oidc_error(&e).await;
                }
                Redirect::to(Urls::Logout.as_ref()).into_response()
            }))
            .layer(oidc_auth_layer);

        protected_routes
            .layer(oidc_login_service)
            .merge(auth_only_routes)
            .merge(oidc_callback_routes)
            .layer(oidc_auth_service)
    } else {
        #[cfg(test)]
        {
            protected_routes
                .merge(auth_only_routes)
                .layer(from_fn(test_auth_middleware))
        }
        #[cfg(not(test))]
        {
            protected_routes.merge(auth_only_routes)
        }
    };

    Ok(app
        .merge(public_routes)
        .layer(TraceLayer::new_for_http())
        .layer(session_layer)
        .with_state(state))
}

fn oidc_redirect_uri(frontend_url: &str) -> Result<Uri, Error> {
    let callback_url = format!(
        "{}{}",
        frontend_url.trim_end_matches('/'),
        Urls::Login.as_ref()
    );

    Uri::from_str(&callback_url)
        .map_err(|err| Error::Configuration(format!("Failed to parse OIDC callback URL: {err:?}")))
}

#[cfg(test)]
/// Builds the web app without performing OIDC discovery.
pub(crate) async fn build_test_app(state: WebState) -> Result<Router, Error> {
    build_app_inner(state, false).await
}

#[cfg(test)]
async fn test_auth_middleware(mut request: Request, next: Next) -> axum::response::Response {
    if let Some(session) = request
        .extensions()
        .get::<tower_sessions::Session>()
        .cloned()
    {
        match session.get::<String>(TEST_OIDC_SESSION_KEY).await {
            Ok(Some(_)) => {
                request
                    .extensions_mut()
                    .insert(crate::web::views::tools::test_user_claims());
            }
            Ok(None) => {}
            Err(err) => error!("Failed to load test auth session: {:?}", err),
        }
    }

    next.run(request).await
}

#[cfg(test)]
/// Creates a cookie header value for a backend-persisted authenticated test session.
pub(crate) async fn test_auth_cookie(state: &WebState) -> String {
    let session_store = get_session_store(&state.db);
    let session = tower_sessions::Session::new(
        None,
        std::sync::Arc::new(session_store),
        Some(Expiry::OnInactivity(Duration::seconds(1800))),
    );
    session
        .insert(TEST_OIDC_SESSION_KEY, "testuser@example.com")
        .await
        .expect("Failed to seed test auth session");
    session
        .save()
        .await
        .expect("Failed to save test auth session");

    format!(
        "id={}",
        session.id().expect("Failed to get test session ID")
    )
}

fn check_certs_exist(
    config_reader: &RwLockReadGuard<'_, Configuration>,
) -> Result<(PathBuf, PathBuf), Error> {
    let cert_file = config_reader.cert_file.clone();
    let cert_key = config_reader.cert_key.clone();
    if !cert_file.exists() {
        return Err(Error::Generic(format!(
            "TLS is enabled but cert_file {cert_file:?} does not exist"
        )));
    }

    if !cert_key.exists() {
        return Err(Error::Generic(format!(
            "TLS is enabled but cert_key {cert_key:?} does not exist"
        )));
    };
    Ok((cert_file, cert_key))
}

/// Start and run the web server
#[cfg(not(tarpaulin_include))]
pub async fn start_web_server(configuration: SendableConfig, app: Router) -> Result<(), Error> {
    use std::net::SocketAddr;

    let configuration_reader = configuration.read().await;

    let listen_address = configuration_reader.listen_addr();
    let (cert_file, cert_key) = check_certs_exist(&configuration_reader)?;
    drop(configuration_reader);

    let tls_config = RustlsConfig::from_pem_file(&cert_file.as_path(), &cert_key.as_path())
        .await
        .map_err(|err| Error::Generic(format!("Failed to load TLS config: {err:?}")))?;
    bind_rustls(
        listen_address.parse::<SocketAddr>().map_err(|err| {
            Error::Generic(format!(
                "Failed to parse listen address {listen_address}: {err:?}"
            ))
        })?,
        tls_config,
    )
    .serve(app.into_make_service())
    .await
    .map_err(|err| Error::Generic(format!("Web server failed: {err:?}")))
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
        "🐕 Starting web server on {} (listen address is {}) 🐕",
        &frontend_url,
        configuration.read().await.listen_addr()
    );

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
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        info!("Web server stopping");
                        return Ok(());
                    },
                    Some(WebServerControl::StopAfter(millis)) => {
                        tokio::time::sleep(tokio::time::Duration::from_millis(millis)).await;
                        info!("Web server stopping");
                        return Ok(());
                    },
                    Some(WebServerControl::Reload) => {
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        info!("Web server reloading");
                    },
                    Some(WebServerControl::ReloadAfter(millis)) => {
                        tokio::time::sleep(tokio::time::Duration::from_secs(millis)).await;
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
    use axum::http::header;
    use entities::host;
    use tower::util::ServiceExt;
    use urls::Urls;

    #[tokio::test]
    async fn test_app_requests() {
        let (db, config) = test_setup().await.expect("Failed to set up test");
        let state = WebState::new(db.clone(), config.clone(), None, None, PathBuf::new());
        let auth_cookie = test_auth_cookie(&state).await;
        let app = build_test_app(state).await.expect("Failed to build app");

        let res = app
            .clone()
            .oneshot(
                axum::http::Request::get(Urls::Index.as_ref())
                    .header(header::COOKIE, &auth_cookie)
                    .body(Body::empty())
                    .expect("failed to build empty body for request"),
            )
            .await
            .expect("Failed to run app");
        assert_eq!(res.status(), StatusCode::OK);

        let host = host::Entity::find()
            .one(db.as_ref())
            .await
            .expect("Failed to query db for host")
            .expect("Failed to find host");

        let url = format!("{}/{}", Urls::Host, host.id);
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::get(&url)
                    .header(header::COOKIE, &auth_cookie)
                    .body(Body::empty())
                    .expect("Failed to get the host ID"),
            )
            .await
            .unwrap_or_else(|err| panic!("Failed to GET {url} {err:?}"));
        assert_eq!(res.status(), StatusCode::OK);

        let service_check = entities::service_check::Entity::find()
            .one(db.as_ref())
            .await
            .expect("Failed to query db for service_check")
            .expect("Failed to find service_check");

        let url = format!("{}/{}", Urls::ServiceCheck, service_check.id);
        let res = app
            .oneshot(
                axum::http::Request::get(&url)
                    .header(header::COOKIE, &auth_cookie)
                    .body(Body::empty())
                    .expect("failed to build empty body for request"),
            )
            .await
            .unwrap_or_else(|err| panic!("Failed to get {url} {err:?}"));
        assert_eq!(res.status(), StatusCode::OK);
    }

    // #[tokio::test]
    // async fn test_not_implemented() {
    //     let (db, config,_dbactor,_tx) = test_setup().await.expect("Failed to set up test");

    //     let res = notimplemented(axum::extract::State(WebState::new(
    //         db,
    //         config.clone(),
    //         None,
    //         None,
    //         PathBuf::new(),
    //     )))
    //     .await;
    //     assert!(res.is_err());
    // }

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

        let _res = OidcErrorHandler::new(None)
            .handle_oidc_error(&MiddlewareError::SessionNotFound)
            .await;

        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        let _res = OidcErrorHandler::new(Some(tx))
            .handle_oidc_error(&MiddlewareError::SessionNotFound)
            .await;
    }

    #[test]
    fn test_oidc_redirect_uri() {
        let redirect_uri =
            oidc_redirect_uri("https://example.com").expect("Failed to build OIDC redirect URI");

        assert_eq!(
            redirect_uri,
            Uri::from_static("https://example.com/auth/login")
        );
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
