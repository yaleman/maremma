//! OIDC Things

use super::WebState;
use crate::prelude::*;

use askama_axum::IntoResponse;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::Uri;
use axum_oidc::AdditionalClaims;
use axum_oidc::OidcClaims;
use axum_oidc::OidcRpInitiatedLogout;

pub async fn logout(
    logout: OidcRpInitiatedLogout,
    State(state): State<WebState>,
) -> Result<impl IntoResponse, (StatusCode, &'static str)> {
    let url: Uri = state.frontend_url.clone().parse().map_err(|err| {
        error!("Failed to parse redirect URL: {:?}", err);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to parse redirect URL",
        )
    })?;
    Ok(logout.with_post_logout_redirect(url).into_response())
}

pub(crate) struct User {
    username: Option<String>,
}

impl User {
    pub fn username(&self) -> String {
        self.username.clone().unwrap_or("Unknown user".to_string())
    }
}

impl<AC> From<OidcClaims<AC>> for User
where
    AC: AdditionalClaims,
{
    fn from(value: OidcClaims<AC>) -> Self {
        Self {
            username: value.preferred_username().map(|u| u.as_str().to_string()),
        }
    }
}
