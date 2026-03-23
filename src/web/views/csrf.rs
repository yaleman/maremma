use crate::constants::SESSION_CSRF_TOKEN;
use crate::web::MaremmaError;

use super::prelude::*;

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct CsrfTokenForm {
    pub(crate) csrf_token: String,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct CsrfRedirectToForm {
    pub(crate) redirect_to: Option<String>,
    pub(crate) csrf_token: String,
}

impl From<Option<String>> for CsrfRedirectToForm {
    fn from(redirect_to: Option<String>) -> Self {
        Self {
            redirect_to,
            csrf_token: String::new(),
        }
    }
}

pub(crate) async fn check_csrf_token(
    csrf_token: &str,
    session: &Session,
) -> Result<(), MaremmaError> {
    let session_csrf_token = session
        .get::<String>(SESSION_CSRF_TOKEN)
        .await
        .map_err(MaremmaError::from)?;

    if session_csrf_token.is_none() {
        debug!("CSRF token not found in session");
        return Err(MaremmaError::CsrfTokenMissing);
    }

    if let Some(token) = &session_csrf_token {
        if token != csrf_token {
            debug!(
                "CSRF token mismatch: session={} form={}",
                &token, csrf_token
            );
            return Err(MaremmaError::CsrfValidationFailed);
        }
    }

    Ok(())
}

pub(crate) async fn issue_csrf_token(
    state: &WebState,
    session: &Session,
) -> Result<String, MaremmaError> {
    let csrf_token = state.new_csrf_token();
    session
        .insert(SESSION_CSRF_TOKEN, &csrf_token)
        .await
        .map_err(MaremmaError::from)?;

    Ok(csrf_token)
}
