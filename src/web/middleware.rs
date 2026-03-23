use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use reqwest::StatusCode;

#[allow(dead_code)]
async fn require_login(
    request: Request,
    next: Next,

    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Response {
    // do something with `request`...
    if claims.is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            "You must be logged in to view this page".to_string(),
        )
            .into_response();
    }

    next.run(request).await
}
