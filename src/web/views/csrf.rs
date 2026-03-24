use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::constants::{CSRF_TOKEN_POOL_LIMIT, CSRF_TOKEN_TTL_MINUTES, SESSION_CSRF_TOKENS};
use crate::web::MaremmaError;

use super::prelude::*;

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct CsrfTokenForm {
    pub(crate) csrf_token: String,
    pub(crate) csrf_scope: String,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct CsrfRedirectToForm {
    pub(crate) redirect_to: Option<String>,
    pub(crate) csrf_token: String,
    pub(crate) csrf_scope: String,
}

impl From<Option<String>> for CsrfRedirectToForm {
    fn from(redirect_to: Option<String>) -> Self {
        Self {
            redirect_to,
            csrf_token: String::new(),
            csrf_scope: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct SessionCsrfTokenEntry {
    token: String,
    scope: String,
    expires_at: i64,
}

fn now_timestamp() -> i64 {
    Utc::now().timestamp()
}

fn expires_at_timestamp() -> i64 {
    (Utc::now() + Duration::minutes(CSRF_TOKEN_TTL_MINUTES)).timestamp()
}

fn prune_entries(entries: &mut Vec<SessionCsrfTokenEntry>, now: i64) {
    entries.retain(|entry| entry.expires_at > now);
    if entries.len() > CSRF_TOKEN_POOL_LIMIT {
        let to_remove = entries.len() - CSRF_TOKEN_POOL_LIMIT;
        entries.drain(..to_remove);
    }
}

async fn load_entries(session: &Session) -> Result<Vec<SessionCsrfTokenEntry>, MaremmaError> {
    session
        .get::<Vec<SessionCsrfTokenEntry>>(SESSION_CSRF_TOKENS)
        .await
        .map(|entries| entries.unwrap_or_default())
        .map_err(MaremmaError::from)
}

async fn save_entries(
    session: &Session,
    entries: &[SessionCsrfTokenEntry],
) -> Result<(), MaremmaError> {
    session
        .insert(SESSION_CSRF_TOKENS, entries)
        .await
        .map_err(MaremmaError::from)
}

fn scope_allowed(submitted_scope: &str, allowed_scopes: &[&str]) -> bool {
    allowed_scopes.contains(&submitted_scope)
}

pub(crate) fn host_scope(host_id: Uuid) -> String {
    format!("host:{host_id}")
}

pub(crate) fn service_scope(service_id: Uuid) -> String {
    format!("service:{service_id}")
}

pub(crate) fn service_check_scope(service_check_id: Uuid) -> String {
    format!("service_check:{service_check_id}")
}

pub(crate) fn host_group_scope(group_id: Uuid) -> String {
    format!("host_group:{group_id}")
}

pub(crate) fn tools_scope() -> &'static str {
    "tools"
}

pub(crate) async fn check_csrf_token(
    csrf_token: &str,
    submitted_scope: &str,
    allowed_scopes: &[&str],
    session: &Session,
) -> Result<(), MaremmaError> {
    if !scope_allowed(submitted_scope, allowed_scopes) {
        debug!("CSRF scope mismatch: submitted={submitted_scope} allowed={allowed_scopes:?}");
        return Err(MaremmaError::CsrfValidationFailed);
    }

    let mut entries = load_entries(session).await?;
    let now = now_timestamp();
    let original_len = entries.len();
    prune_entries(&mut entries, now);
    if entries.len() != original_len {
        save_entries(session, &entries).await?;
    }

    if entries.is_empty() {
        debug!("CSRF token not found in session");
        return Err(MaremmaError::CsrfTokenMissing);
    }

    if entries
        .iter()
        .any(|entry| entry.token == csrf_token && entry.scope == submitted_scope)
    {
        return Ok(());
    }

    debug!(
        "CSRF token mismatch: token={} submitted_scope={}",
        csrf_token, submitted_scope
    );
    Err(MaremmaError::CsrfValidationFailed)
}

pub(crate) async fn consume_csrf_token(
    csrf_token: &str,
    submitted_scope: &str,
    allowed_scopes: &[&str],
    session: &Session,
) -> Result<(), MaremmaError> {
    if !scope_allowed(submitted_scope, allowed_scopes) {
        debug!("CSRF scope mismatch during consume: submitted={submitted_scope}");
        return Err(MaremmaError::CsrfValidationFailed);
    }

    let mut entries = load_entries(session).await?;
    let now = now_timestamp();
    prune_entries(&mut entries, now);
    let original_len = entries.len();
    entries.retain(|entry| !(entry.token == csrf_token && entry.scope == submitted_scope));

    if entries.len() == original_len {
        if entries.is_empty() {
            return Err(MaremmaError::CsrfTokenMissing);
        }
        return Err(MaremmaError::CsrfValidationFailed);
    }

    save_entries(session, &entries).await
}

pub(crate) async fn issue_csrf_token(
    session: &Session,
    scope: &str,
) -> Result<String, MaremmaError> {
    let mut entries = load_entries(session).await?;
    prune_entries(&mut entries, now_timestamp());

    let csrf_token = Uuid::new_v4().to_string();
    entries.push(SessionCsrfTokenEntry {
        token: csrf_token.clone(),
        scope: scope.to_string(),
        expires_at: expires_at_timestamp(),
    });
    prune_entries(&mut entries, now_timestamp());
    save_entries(session, &entries).await?;

    Ok(csrf_token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::SESSION_CSRF_TOKENS;

    fn expired_entry(token: &str, scope: &str) -> SessionCsrfTokenEntry {
        SessionCsrfTokenEntry {
            token: token.to_string(),
            scope: scope.to_string(),
            expires_at: now_timestamp() - 1,
        }
    }

    async fn stored_entries(session: &Session) -> Vec<SessionCsrfTokenEntry> {
        session
            .get::<Vec<SessionCsrfTokenEntry>>(SESSION_CSRF_TOKENS)
            .await
            .expect("Failed to load CSRF entries")
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn test_issue_multiple_tokens_for_same_scope() {
        let state = WebState::test().await;
        let session = state.get_session();
        let scope = host_scope(Uuid::new_v4());

        let token_one = issue_csrf_token(&session, &scope)
            .await
            .expect("Failed to issue first token");
        let token_two = issue_csrf_token(&session, &scope)
            .await
            .expect("Failed to issue second token");

        assert_ne!(token_one, token_two);
        check_csrf_token(&token_one, &scope, &[scope.as_str()], &session)
            .await
            .expect("First token should validate");
        check_csrf_token(&token_two, &scope, &[scope.as_str()], &session)
            .await
            .expect("Second token should validate");
    }

    #[tokio::test]
    async fn test_issue_tokens_for_different_scopes() {
        let state = WebState::test().await;
        let session = state.get_session();
        let host_page = host_scope(Uuid::new_v4());
        let service_page = service_scope(Uuid::new_v4());

        let host_token = issue_csrf_token(&session, &host_page)
            .await
            .expect("Failed to issue host token");
        let service_token = issue_csrf_token(&session, &service_page)
            .await
            .expect("Failed to issue service token");

        check_csrf_token(&host_token, &host_page, &[host_page.as_str()], &session)
            .await
            .expect("Host token should validate");
        check_csrf_token(
            &service_token,
            &service_page,
            &[service_page.as_str()],
            &session,
        )
        .await
        .expect("Service token should validate");
    }

    #[tokio::test]
    async fn test_reject_expired_tokens() {
        let state = WebState::test().await;
        let session = state.get_session();
        let scope = tools_scope().to_string();
        session
            .insert(
                SESSION_CSRF_TOKENS,
                vec![expired_entry("expired-token", &scope)],
            )
            .await
            .expect("Failed to seed CSRF entries");

        let err = check_csrf_token("expired-token", &scope, &[scope.as_str()], &session)
            .await
            .expect_err("Expired token should fail");
        assert_eq!(err, MaremmaError::CsrfTokenMissing);
        assert!(stored_entries(&session).await.is_empty());
    }

    #[tokio::test]
    async fn test_prunes_expired_tokens_during_issue_and_validate() {
        let state = WebState::test().await;
        let session = state.get_session();
        let scope = tools_scope().to_string();
        session
            .insert(
                SESSION_CSRF_TOKENS,
                vec![expired_entry("expired-token", &scope)],
            )
            .await
            .expect("Failed to seed expired token");

        let new_token = issue_csrf_token(&session, &scope)
            .await
            .expect("Failed to issue replacement token");
        let entries = stored_entries(&session).await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].token, new_token);

        check_csrf_token(&new_token, &scope, &[scope.as_str()], &session)
            .await
            .expect("Fresh token should validate");
    }

    #[tokio::test]
    async fn test_rejects_scope_mismatch() {
        let state = WebState::test().await;
        let session = state.get_session();
        let host_page = host_scope(Uuid::new_v4());
        let service_page = service_scope(Uuid::new_v4());
        let token = issue_csrf_token(&session, &host_page)
            .await
            .expect("Failed to issue token");

        let err = check_csrf_token(&token, &host_page, &[service_page.as_str()], &session)
            .await
            .expect_err("Mismatched scope should fail");
        assert_eq!(err, MaremmaError::CsrfValidationFailed);
    }

    #[tokio::test]
    async fn test_consume_one_token_without_affecting_others() {
        let state = WebState::test().await;
        let session = state.get_session();
        let scope = host_scope(Uuid::new_v4());
        let token_one = issue_csrf_token(&session, &scope)
            .await
            .expect("Failed to issue first token");
        let token_two = issue_csrf_token(&session, &scope)
            .await
            .expect("Failed to issue second token");

        consume_csrf_token(&token_one, &scope, &[scope.as_str()], &session)
            .await
            .expect("Failed to consume first token");

        check_csrf_token(&token_two, &scope, &[scope.as_str()], &session)
            .await
            .expect("Second token should remain valid");
        assert_eq!(
            check_csrf_token(&token_one, &scope, &[scope.as_str()], &session)
                .await
                .expect_err("Consumed token should be invalid"),
            MaremmaError::CsrfValidationFailed
        );
    }
}
