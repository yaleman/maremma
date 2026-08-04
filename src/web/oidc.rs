//! OIDC Things

use super::urls::Urls;
use super::WebState;
use crate::prelude::*;

use axum::extract::State;
use axum::http::{StatusCode, Uri};
use axum::response::Redirect;
use axum_oidc::{AdditionalClaims, OidcClaims, OidcRpInitiatedLogout};
use tower_sessions::Session;

pub(crate) const POST_LOGIN_RETURN_PATH: &str = "maremma-post-login-return-path";

/// Return an application-local redirect target, rejecting absolute and scheme-relative URLs.
pub(crate) fn valid_return_path(candidate: &str) -> Option<String> {
    let uri: Uri = candidate.parse().ok()?;
    let path_and_query = uri.path_and_query()?;

    (uri.scheme().is_none()
        && uri.authority().is_none()
        && path_and_query.as_str() == candidate
        && uri.path().starts_with('/')
        && !uri.path().starts_with("//"))
    .then(|| candidate.to_string())
}

/// Complete login by returning the visitor to their original in-app destination.
pub(crate) async fn login(session: Session) -> Result<Redirect, MaremmaError> {
    let return_path: Option<String> = session.remove(POST_LOGIN_RETURN_PATH).await?;
    let target = return_path
        .as_deref()
        .and_then(valid_return_path)
        .unwrap_or_else(|| Urls::Index.as_ref().to_string());

    Ok(Redirect::to(&target))
}

/// Logs the user out
pub async fn logout(session: Session) -> Result<Redirect, (StatusCode, &'static str)> {
    session.clear().await;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    Ok(Redirect::to(Urls::Index.as_ref()))
}

/// Takes the logout action from the OIDC provider and logs the user out
#[cfg(not(tarpaulin_include))] // Can't test this because we can't create the `OidcRpInitiatedLogout` object
#[instrument(level = "info", skip_all, fields(post_logout_redirect_uri=?logout.uri()))]
pub async fn rp_logout(
    State(state): State<WebState>,
    session: Session,
    logout: OidcRpInitiatedLogout,
) -> Result<OidcRpInitiatedLogout, (StatusCode, &'static str)> {
    session.clear().await;

    let url: Uri = state
        .configuration
        .read()
        .await
        .frontend_url
        .parse()
        .map_err(|err| {
            error!("Failed to parse redirect URL: {:?}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to parse redirect URL, your session has been cleared on our end.",
            )
        })?;
    Ok(logout.with_post_logout_redirect(url))
}

#[derive(Debug)]
pub(crate) struct User {
    username: String,
}

impl User {
    pub fn username(&self) -> String {
        self.username.to_owned()
    }
}

impl<AC> From<OidcClaims<AC>> for User
where
    AC: AdditionalClaims,
{
    fn from(value: OidcClaims<AC>) -> Self {
        let username = match value.preferred_username() {
            Some(username) => username.as_str().to_string(),
            None => value.subject().as_str().to_string(),
        };

        Self { username }
    }
}

#[cfg(test)]
mod tests {

    use std::path::PathBuf;
    use std::sync::Arc;

    use axum::http::header;
    use axum::response::IntoResponse;
    use tower::ServiceExt;

    use crate::db::tests::test_setup;
    use crate::web::urls::Urls;
    use crate::web::{build_test_app, test_auth_cookie};

    use super::*;

    #[tokio::test]
    async fn test_logout_view() {
        use tower_sessions::MemoryStore;

        let _ = test_setup().await.expect("Failed to setup test");

        let store = MemoryStore::default();
        let session = tower_sessions::Session::new(None, Arc::new(store), None);
        let res = logout(session).await;

        assert!(res.is_ok());
        let res = res.expect("Errored out").into_response();

        assert_eq!(res.status(), axum::http::StatusCode::SEE_OTHER);
        assert_eq!(
            res.headers()
                .get("location")
                .expect("Failed to get location header"),
            Urls::Index.as_ref()
        );
    }

    #[tokio::test]
    async fn test_login_view_defaults_to_index() {
        use tower_sessions::MemoryStore;

        let session = tower_sessions::Session::new(None, Arc::new(MemoryStore::default()), None);
        let res = login(session)
            .await
            .expect("Failed to complete login")
            .into_response();

        assert_eq!(res.status(), axum::http::StatusCode::SEE_OTHER);
        assert_eq!(
            res.headers()
                .get("location")
                .expect("Failed to get location header"),
            Urls::Index.as_ref()
        );
    }

    #[tokio::test]
    async fn test_login_view_returns_to_saved_path_once() {
        use tower_sessions::MemoryStore;

        let session = tower_sessions::Session::new(None, Arc::new(MemoryStore::default()), None);
        session
            .insert(POST_LOGIN_RETURN_PATH, "/hosts?search=maremma")
            .await
            .expect("Failed to save return path");

        let res = login(session.clone())
            .await
            .expect("Failed to complete login")
            .into_response();

        assert_eq!(
            res.headers()
                .get("location")
                .expect("Failed to get location header"),
            "/hosts?search=maremma"
        );
        assert_eq!(
            session
                .get::<String>(POST_LOGIN_RETURN_PATH)
                .await
                .expect("Failed to load return path"),
            None
        );
    }

    #[test]
    fn test_valid_return_path_rejects_external_urls() {
        assert_eq!(
            valid_return_path("/hosts?search=maremma"),
            Some("/hosts?search=maremma".to_string())
        );
        assert_eq!(valid_return_path("https://example.com"), None);
        assert_eq!(valid_return_path("//example.com"), None);
    }

    #[tokio::test]
    async fn test_login_view_rejects_unsafe_saved_path() {
        use tower_sessions::MemoryStore;

        let session = tower_sessions::Session::new(None, Arc::new(MemoryStore::default()), None);
        session
            .insert(POST_LOGIN_RETURN_PATH, "https://example.com")
            .await
            .expect("Failed to save return path");

        let res = login(session)
            .await
            .expect("Failed to complete login")
            .into_response();

        assert_eq!(
            res.headers()
                .get("location")
                .expect("Failed to get location header"),
            Urls::Index.as_ref()
        );
    }

    #[tokio::test]
    async fn test_logout() {
        let (db, config) = test_setup().await.expect("Failed to setup test");
        let state = crate::web::WebState::new(db.clone(), config, None, None, PathBuf::new());
        let auth_cookie = test_auth_cookie(&state).await;

        let app = build_test_app(state).await.expect("Failed to build app");

        let res = app
            .oneshot(
                axum::http::Request::get(Urls::Logout.as_ref())
                    .header(header::COOKIE, &auth_cookie)
                    .body(axum::body::Body::empty())
                    .expect("Failed to build request"),
            )
            .await;

        assert!(res.is_ok());

        let res = res.expect("Errored out");

        assert_eq!(res.status(), axum::http::StatusCode::SEE_OTHER);
    }
}
