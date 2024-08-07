//! OIDC Things

use askama_axum::IntoResponse;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::Uri;
use axum_oidc::OidcRpInitiatedLogout;
use tracing::error;

use super::WebState;

pub async fn logout(
    logout: OidcRpInitiatedLogout,
    State(state): State<WebState>,
) -> Result<impl IntoResponse, (StatusCode, &'static str)> {
    #[allow(clippy::expect_used)]
    let url: Uri = state.frontend_url.clone().parse().map_err(|err| {
        error!("Failed to parse redirect URL: {:?}", err);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to parse redirect URL",
        )
    })?;
    Ok(logout.with_post_logout_redirect(url).into_response())
}
