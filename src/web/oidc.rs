//! OIDC Things

use super::WebState;
use crate::prelude::*;

use askama_axum::IntoResponse;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::Uri;
use axum::response::Redirect;
use axum_oidc::AdditionalClaims;
use axum_oidc::OidcClaims;
use axum_oidc::OidcRpInitiatedLogout;
use tower_sessions::Session;

/// Logs the user out
pub async fn logout(session: Session) -> Result<Redirect, (StatusCode, &'static str)> {
    session.clear().await;
    Ok(Redirect::to("/"))
}

/// Takes the logout action from the OIDC provider and logs the user out
#[cfg(not(tarpaulin_include))] // Can't test this because we can't create the `OidcRpInitiatedLogout` object
#[instrument(level = "info", skip_all, fields(post_logout_redirect_uri=?logout.uri()))]
pub async fn rp_logout(
    State(state): State<WebState>,
    session: Session,
    logout: OidcRpInitiatedLogout,
) -> Result<impl IntoResponse, (StatusCode, &'static str)> {
    session.clear().await;

    let url: Uri = state.frontend_url.parse().map_err(|err| {
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
