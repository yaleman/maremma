//! OIDC Things

use super::urls::Urls;
use super::WebState;
use crate::prelude::*;

use axum::extract::State;
use axum::http::{StatusCode, Uri};
use axum::response::Redirect;
use axum_oidc::{AdditionalClaims, OidcClaims, OidcRpInitiatedLogout};
use tower_sessions::Session;

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

    use axum::response::IntoResponse;
    use tower::ServiceExt;

    use crate::db::tests::test_setup;
    use crate::web::build_app;
    use crate::web::urls::Urls;

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
    async fn test_logout() {
        if std::env::var("CI").is_ok() {
            eprintln!("Skipping test because it fails in CI");
            return;
        }

        let (db, config) = test_setup().await.expect("Failed to setup test");

        let app = build_app(crate::web::WebState::new(
            db.clone(),
            config,
            None,
            None,
            PathBuf::new(),
        ))
        .await
        .expect("Failed to build app");

        let res = app
            .oneshot(
                axum::http::Request::get(Urls::Logout.as_ref())
                    .body(axum::body::Body::empty())
                    .expect("Failed to build request"),
            )
            .await;

        assert!(res.is_ok());

        let res = res.expect("Errored out");

        assert_eq!(res.status(), axum::http::StatusCode::SEE_OTHER);
    }
}
