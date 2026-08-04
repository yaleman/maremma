use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use axum_oidc::OidcAccessToken;
use tower_sessions::Session;

use crate::errors::MaremmaError;
use crate::web::oidc::{self, POST_LOGIN_RETURN_PATH};
use crate::web::urls::Urls;

/// Redirect unauthenticated requests through the fixed OIDC callback route.
pub(crate) async fn require_login(request: Request, next: Next) -> Result<Response, MaremmaError> {
    if request.extensions().get::<OidcAccessToken>().is_some() {
        return Ok(next.run(request).await);
    }

    let session = request
        .extensions()
        .get::<Session>()
        .cloned()
        .ok_or_else(|| MaremmaError::Session("Session middleware is not available".to_string()))?;

    match request
        .uri()
        .path_and_query()
        .and_then(|path_and_query| oidc::valid_return_path(path_and_query.as_str()))
    {
        Some(return_path) => session.insert(POST_LOGIN_RETURN_PATH, return_path).await?,
        None => {
            let _: Option<String> = session.remove(POST_LOGIN_RETURN_PATH).await?;
        }
    }

    Ok(Redirect::to(Urls::Login.as_ref()).into_response())
}
