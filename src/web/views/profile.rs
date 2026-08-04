use axum::{
    extract::State,
    response::{Redirect, Response},
    Form,
};
use chrono::{DateTime, Duration, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use serde::Deserialize;

use crate::db::entities::api_token;
use crate::errors::MaremmaError;
use crate::web::views::csrf::{
    api_tokens_scope, check_csrf_token, consume_csrf_token, issue_csrf_token,
};

use super::prelude::*;

const MAX_TOKEN_LIFETIME_HOURS: i64 = 24 * 90;
const MAX_TOKEN_NAME_LENGTH: usize = 64;

#[derive(Clone, Debug)]
pub(crate) struct ApiTokenTemplateData {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) issued_at: DateTime<Utc>,
    pub(crate) expires_at: DateTime<Utc>,
}

#[derive(Template, WebTemplate)]
#[template(path = "profile.html")]
pub(crate) struct ProfileTemplate {
    title: String,
    username: Option<String>, // for the header
    profile_user: User,
    api_tokens: Vec<ApiTokenTemplateData>,
    csrf_token: String,
    csrf_scope: String,
    issued_token: Option<String>,
    error_message: Option<String>,
}

pub(crate) struct ProfileFormResponse {
    template: ProfileTemplate,
    status: StatusCode,
}

impl IntoResponse for ProfileFormResponse {
    fn into_response(self) -> Response {
        let mut response = self.template.into_response();
        *response.status_mut() = self.status;
        response
    }
}

#[derive(Deserialize)]
pub(crate) struct CreateApiTokenForm {
    name: String,
    expires_in_hours: i64,
    csrf_token: String,
    csrf_scope: String,
}

#[derive(Deserialize)]
pub(crate) struct RevokeApiTokenForm {
    csrf_token: String,
    csrf_scope: String,
}

async fn active_tokens(
    state: &WebState,
    subject: &str,
) -> Result<Vec<ApiTokenTemplateData>, MaremmaError> {
    api_token::Entity::find()
        .filter(api_token::Column::Subject.eq(subject))
        .filter(api_token::Column::RevokedAt.is_null())
        .filter(api_token::Column::ExpiresAt.gt(Utc::now()))
        .order_by_desc(api_token::Column::IssuedAt)
        .all(state.db())
        .await
        .map(|tokens| {
            tokens
                .into_iter()
                .map(|token| ApiTokenTemplateData {
                    id: token.id,
                    name: token.name,
                    issued_at: token.issued_at,
                    expires_at: token.expires_at,
                })
                .collect()
        })
        .map_err(MaremmaError::from)
}

async fn profile_template(
    state: &WebState,
    session: &Session,
    user: User,
    issued_token: Option<String>,
    error_message: Option<String>,
) -> Result<ProfileTemplate, MaremmaError> {
    let csrf_scope = api_tokens_scope().to_string();
    let csrf_token = issue_csrf_token(session, &csrf_scope).await?;
    let api_tokens = active_tokens(state, user.subject()).await?;

    Ok(ProfileTemplate {
        title: user.username(),
        username: Some(user.username()),
        profile_user: user,
        api_tokens,
        csrf_token,
        csrf_scope,
        issued_token,
        error_message,
    })
}

pub(crate) async fn profile(
    State(state): State<WebState>,
    session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<ProfileTemplate, MaremmaError> {
    let user = check_login(claims)?;

    profile_template(&state, &session, user, None, None).await
}

pub(crate) async fn create_api_token(
    State(state): State<WebState>,
    session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    Form(form): Form<CreateApiTokenForm>,
) -> Result<ProfileFormResponse, MaremmaError> {
    let user = check_login(claims)?;
    let allowed_scopes = [api_tokens_scope()];
    check_csrf_token(
        &form.csrf_token,
        &form.csrf_scope,
        &allowed_scopes,
        &session,
    )
    .await?;

    let name = form.name.trim();
    if name.is_empty() || name.len() > MAX_TOKEN_NAME_LENGTH {
        let error_message =
            format!("Token names must contain between 1 and {MAX_TOKEN_NAME_LENGTH} characters");
        return Ok(ProfileFormResponse {
            template: profile_template(&state, &session, user, None, Some(error_message)).await?,
            status: StatusCode::BAD_REQUEST,
        });
    }
    if !(1..=MAX_TOKEN_LIFETIME_HOURS).contains(&form.expires_in_hours) {
        let error_message =
            format!("Token lifetime must be between 1 and {MAX_TOKEN_LIFETIME_HOURS} hours");
        return Ok(ProfileFormResponse {
            template: profile_template(&state, &session, user, None, Some(error_message)).await?,
            status: StatusCode::BAD_REQUEST,
        });
    }

    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::hours(form.expires_in_hours);
    let token_id = Uuid::new_v4();
    let encoded_token = state
        .auth()?
        .issue(user.subject().to_string(), token_id, issued_at, expires_at)
        .map_err(|_| MaremmaError::Configuration("Failed to issue API token".to_string()))?;

    api_token::ActiveModel {
        id: Set(token_id),
        subject: Set(user.subject().to_string()),
        name: Set(name.to_string()),
        issued_at: Set(issued_at),
        expires_at: Set(expires_at),
        revoked_at: Set(None),
    }
    .insert(state.db())
    .await
    .map_err(MaremmaError::from)?;

    consume_csrf_token(
        &form.csrf_token,
        &form.csrf_scope,
        &allowed_scopes,
        &session,
    )
    .await?;

    Ok(ProfileFormResponse {
        template: profile_template(&state, &session, user, Some(encoded_token), None).await?,
        status: StatusCode::OK,
    })
}

pub(crate) async fn revoke_api_token(
    Path(token_id): Path<Uuid>,
    State(state): State<WebState>,
    session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    Form(form): Form<RevokeApiTokenForm>,
) -> Result<Redirect, MaremmaError> {
    let user = check_login(claims)?;
    let allowed_scopes = [api_tokens_scope()];
    check_csrf_token(
        &form.csrf_token,
        &form.csrf_scope,
        &allowed_scopes,
        &session,
    )
    .await?;

    let token = api_token::Entity::find_by_id(token_id)
        .one(state.db())
        .await
        .map_err(MaremmaError::from)?
        .ok_or(MaremmaError::Unauthorized)?;
    if token.subject != user.subject() || token.revoked_at.is_some() {
        return Err(MaremmaError::Unauthorized);
    }

    let mut token = token.into_active_model();
    token.revoked_at.set_if_not_equals(Some(Utc::now()));
    token.update(state.db()).await.map_err(MaremmaError::from)?;
    consume_csrf_token(
        &form.csrf_token,
        &form.csrf_scope,
        &allowed_scopes,
        &session,
    )
    .await?;

    Ok(Redirect::to(Urls::Profile.as_ref()))
}

#[cfg(test)]
mod tests {
    use crate::db::entities::api_token;
    use crate::web::views::csrf::{api_tokens_scope, issue_csrf_token};

    use axum::Form;
    use sea_orm::EntityTrait;

    use super::*;

    #[tokio::test]
    async fn test_view_profile() {
        use super::*;
        let state = WebState::test().await;

        let res = super::profile(
            State(state.clone()),
            state.get_session(),
            Some(crate::web::views::tools::test_user_claims()),
        )
        .await;
        let res_body = res.expect("Failed to get response").to_string();
        assert!(res_body.contains("testuser@example.com"));
        let res = super::profile(
            State(state.clone()),
            state.get_session(),
            Some(crate::web::views::tools::test_user_claims()),
        )
        .await;
        assert_eq!(
            res.expect("Failed to get response")
                .into_response()
                .status(),
            StatusCode::OK
        )
    }

    #[tokio::test]
    async fn test_view_profile_noauth() {
        use super::*;
        let state = WebState::test().await;

        let res = super::profile(State(state.clone()), state.get_session(), None).await;

        assert!(res.is_err());
        assert_eq!(res.into_response().status(), StatusCode::UNAUTHORIZED)
    }

    #[tokio::test]
    async fn test_create_api_token_renders_the_raw_token_once() {
        let state = WebState::test().await;
        let session = state.get_session();
        let csrf_scope = api_tokens_scope().to_string();
        let csrf_token = issue_csrf_token(&session, &csrf_scope)
            .await
            .expect("Failed to issue CSRF token");

        let response = create_api_token(
            State(state.clone()),
            session.clone(),
            Some(crate::web::views::tools::test_user_claims()),
            Form(CreateApiTokenForm {
                name: "automation".to_string(),
                expires_in_hours: 24,
                csrf_token,
                csrf_scope,
            }),
        )
        .await
        .expect("Failed to create API token");

        assert!(response.template.issued_token.is_some());
        assert!(response
            .template
            .to_string()
            .contains("Copy this token now"));
        let subsequent_response = profile(
            State(state.clone()),
            session,
            Some(crate::web::views::tools::test_user_claims()),
        )
        .await
        .expect("Failed to render subsequent profile response");
        assert!(!subsequent_response
            .to_string()
            .contains("Copy this token now"));
        assert_eq!(
            api_token::Entity::find()
                .all(state.db())
                .await
                .expect("Failed to load API tokens")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn test_create_api_token_enforces_lifetime_bounds() {
        let state = WebState::test().await;
        let session = state.get_session();
        let csrf_scope = api_tokens_scope().to_string();

        for expires_in_hours in [0, MAX_TOKEN_LIFETIME_HOURS + 1] {
            let csrf_token = issue_csrf_token(&session, &csrf_scope)
                .await
                .expect("Failed to issue CSRF token");
            let result = create_api_token(
                State(state.clone()),
                session.clone(),
                Some(crate::web::views::tools::test_user_claims()),
                Form(CreateApiTokenForm {
                    name: "out-of-bounds".to_string(),
                    expires_in_hours,
                    csrf_token,
                    csrf_scope: csrf_scope.clone(),
                }),
            )
            .await;
            let response = result.expect("Invalid token lifetime should render the profile");
            assert_eq!(response.status, StatusCode::BAD_REQUEST);
            assert!(response
                .template
                .error_message
                .as_deref()
                .is_some_and(|message| message.contains("Token lifetime")));
            assert!(response
                .template
                .to_string()
                .contains("Token lifetime must be between"));
        }
        assert!(api_token::Entity::find()
            .all(state.db())
            .await
            .expect("Failed to load API tokens")
            .is_empty());
    }

    #[tokio::test]
    async fn test_create_api_token_rejects_an_empty_name_with_bad_request() {
        let state = WebState::test().await;
        let session = state.get_session();
        let csrf_scope = api_tokens_scope().to_string();
        let csrf_token = issue_csrf_token(&session, &csrf_scope)
            .await
            .expect("Failed to issue CSRF token");

        let response = create_api_token(
            State(state.clone()),
            session,
            Some(crate::web::views::tools::test_user_claims()),
            Form(CreateApiTokenForm {
                name: "   ".to_string(),
                expires_in_hours: 24,
                csrf_token,
                csrf_scope,
            }),
        )
        .await
        .expect("Invalid token name should render the profile");

        assert_eq!(response.status, StatusCode::BAD_REQUEST);
        assert!(response
            .template
            .to_string()
            .contains("Token names must contain between"));
        assert!(api_token::Entity::find()
            .all(state.db())
            .await
            .expect("Failed to load API tokens")
            .is_empty());
    }

    #[tokio::test]
    async fn test_revoke_api_token_marks_own_token_revoked() {
        let state = WebState::test().await;
        let session = state.get_session();
        let csrf_scope = api_tokens_scope().to_string();
        let csrf_token = issue_csrf_token(&session, &csrf_scope)
            .await
            .expect("Failed to issue CSRF token");

        let created = create_api_token(
            State(state.clone()),
            session.clone(),
            Some(crate::web::views::tools::test_user_claims()),
            Form(CreateApiTokenForm {
                name: "revoke-me".to_string(),
                expires_in_hours: 24,
                csrf_token,
                csrf_scope: csrf_scope.clone(),
            }),
        )
        .await
        .expect("Failed to create API token");
        let token_id = created.template.api_tokens[0].id;

        let revoke_csrf_token = issue_csrf_token(&session, &csrf_scope)
            .await
            .expect("Failed to issue revocation CSRF token");
        let response = revoke_api_token(
            Path(token_id),
            State(state.clone()),
            session,
            Some(crate::web::views::tools::test_user_claims()),
            Form(RevokeApiTokenForm {
                csrf_token: revoke_csrf_token,
                csrf_scope,
            }),
        )
        .await
        .expect("Failed to revoke API token")
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let token = api_token::Entity::find_by_id(token_id)
            .one(state.db())
            .await
            .expect("Failed to retrieve API token")
            .expect("API token should exist");
        assert!(token.revoked_at.is_some());
        assert!(!token.is_active_at(Utc::now()));
    }

    #[tokio::test]
    async fn test_revoke_api_token_rejects_another_user() {
        let state = WebState::test().await;
        let session = state.get_session();
        let csrf_scope = api_tokens_scope().to_string();
        let csrf_token = issue_csrf_token(&session, &csrf_scope)
            .await
            .expect("Failed to issue CSRF token");
        let created = create_api_token(
            State(state.clone()),
            session.clone(),
            Some(crate::web::views::tools::test_user_claims()),
            Form(CreateApiTokenForm {
                name: "other-user-token".to_string(),
                expires_in_hours: 24,
                csrf_token,
                csrf_scope: csrf_scope.clone(),
            }),
        )
        .await
        .expect("Failed to create API token");
        let token_id = created.template.api_tokens[0].id;

        let revoke_csrf_token = issue_csrf_token(&session, &csrf_scope)
            .await
            .expect("Failed to issue revocation CSRF token");
        let result = revoke_api_token(
            Path(token_id),
            State(state.clone()),
            session,
            Some(crate::web::views::tools::test_user_claims_for(
                "other-user@example.com",
            )),
            Form(RevokeApiTokenForm {
                csrf_token: revoke_csrf_token,
                csrf_scope,
            }),
        )
        .await;

        assert!(matches!(result, Err(MaremmaError::Unauthorized)));
        let token = api_token::Entity::find_by_id(token_id)
            .one(state.db())
            .await
            .expect("Failed to retrieve API token")
            .expect("API token should exist");
        assert!(token.revoked_at.is_none());
    }
}
