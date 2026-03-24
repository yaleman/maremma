use super::prelude::*;
use crate::constants::{SESSION_CSRF_SCOPE, SESSION_CSRF_TOKEN};
use crate::db::update_db_from_config;
use crate::web::views::csrf::{
    check_csrf_token, consume_csrf_token, issue_csrf_token, tools_scope, CsrfTokenForm,
};
use crate::web::{Configuration, MaremmaError};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue};
use axum::Form;
use sea_orm::prelude::Expr;
use tokio::sync::RwLock;

#[cfg(test)]
use openidconnect::{IssuerUrl, StandardClaims, SubjectIdentifier};
#[cfg(test)]
use reqwest::Url;
#[cfg(test)]
use std::str::FromStr;

#[derive(Template, Debug, WebTemplate)]
#[template(path = "tools.html")]
pub(crate) struct ToolsTemplate {
    title: String,
    username: Option<String>,
    message: Option<String>,
    status: ActionStatus,
    csrf_token: String,
    csrf_scope: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FormAction {
    SetAllToUrgent,
    ReloadConfig,
}

impl std::fmt::Display for FormAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormAction::SetAllToUrgent => write!(f, "Set all to urgent"),
            FormAction::ReloadConfig => write!(f, "Reload config"),
        }
    }
}

impl AsRef<str> for FormAction {
    fn as_ref(&self) -> &str {
        match self {
            FormAction::SetAllToUrgent => "set_all_to_urgent",
            FormAction::ReloadConfig => "reload_config",
        }
    }
}

#[derive(Deserialize, Default, Debug, Copy, Clone)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ActionStatus {
    Success,
    Error,
    #[default]
    Unknown,
}

impl std::fmt::Display for ActionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionStatus::Success => write!(f, "success"),
            ActionStatus::Error => write!(f, "error"),
            ActionStatus::Unknown => write!(f, "unknown"),
        }
    }
}

impl ActionStatus {
    pub(crate) fn alert_classes(self) -> &'static str {
        match self {
            ActionStatus::Success => "app-alert app-alert-success",
            ActionStatus::Error => "app-alert app-alert-danger",
            ActionStatus::Unknown => "app-alert app-alert-info",
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct ToolsForm {
    action: Option<FormAction>,
    csrf_token: Option<String>,
    csrf_scope: Option<String>,
}
#[derive(Deserialize, Default)]
pub(crate) struct ToolsQuery {
    result: Option<String>,
    #[serde(default)]
    status: ActionStatus,
}

#[instrument(level = "info", skip_all)]
async fn tools_reload_config(state: &WebState) -> Result<(), MaremmaError> {
    info!("Asked to reload config");

    let new_config = Configuration::new(&state.config_filepath)
        .await
        .map_err(|e| {
            error!("Failed to reload config: {:?}", e);
            e
        })?;

    *state.configuration.write().await = new_config;

    let new_config = Configuration::new(&state.config_filepath)
        .await
        .map_err(|e| {
            error!("Failed to reload config: {:?}", e);
            e
        })?;
    update_db_from_config(state.db(), Arc::new(RwLock::new(new_config)))
        .await
        .map_err(|e| {
            error!("Failed to reload config: {:?}", e);
            e
        })?;

    info!("Reloaded config");
    Ok(())
}

/// Seen at `/tools`
pub(crate) async fn tools(
    State(state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    Query(results): Query<ToolsQuery>,
    session: Session,
    Form(form): Form<ToolsForm>,
) -> Result<ToolsTemplate, impl IntoResponse> {
    if claims.is_none() {
        // TODO: check that the user is an admin
        return Err(MaremmaError::Unauthorized.into_response());
    }

    if let (Some(action), Some(csrf_token), Some(csrf_scope)) =
        (&form.action, &form.csrf_token, &form.csrf_scope)
    {
        let allowed_scopes = [tools_scope()];
        // pull the CSRF token from the session store
        check_csrf_token(csrf_token, csrf_scope, &allowed_scopes, &session)
            .await
            .map_err(|e| e.into_response())?;

        match action {
            FormAction::SetAllToUrgent => {
                info!("Asked to set all to urgent");
                let db_lock = state.db();
                entities::service_check::Entity::update_many()
                    .col_expr(
                        entities::service_check::Column::Status,
                        Expr::value(ServiceStatus::Urgent),
                    )
                    .exec(db_lock)
                    .await
                    .map_err(|e| {
                        error!("Failed to set all to urgent: {:?}", e);
                        Redirect::to(&format!(
                            "{}?result=Failed to set all tasks to urgent&status={}",
                            Urls::Tools,
                            ActionStatus::Error,
                        ))
                        .into_response()
                    })?;
                consume_csrf_token(csrf_token, csrf_scope, &allowed_scopes, &session)
                    .await
                    .map_err(|e| e.into_response())?;
                return Err(Redirect::to(&format!(
                    "{}?result=Set all tasks to urgent&status={}",
                    Urls::Tools,
                    ActionStatus::Success,
                ))
                .into_response());
            }
            FormAction::ReloadConfig => {
                if let Err(err) = tools_reload_config(&state).await {
                    error!("Failed to reload config: {:?}", err);
                    return Err(Redirect::to(&format!(
                        "{}?result=Failed to reload config&status={}",
                        Urls::Tools,
                        ActionStatus::Error,
                    ))
                    .into_response());
                }
                consume_csrf_token(csrf_token, csrf_scope, &allowed_scopes, &session)
                    .await
                    .map_err(|e| e.into_response())?;
                return Err(Redirect::to(&format!(
                    "{}?result=Reloaded config&status={}",
                    Urls::Tools,
                    ActionStatus::Success,
                ))
                .into_response());
            }
        }
    }
    let csrf_scope = tools_scope().to_string();
    let csrf_token = issue_csrf_token(&session, &csrf_scope)
        .await
        .map_err(|err| err.into_response())?;

    Ok(ToolsTemplate {
        title: "Tools".to_string(),
        username: claims.map(|c: OidcClaims<EmptyAdditionalClaims>| User::from(c).username()),
        message: results.result,
        status: results.status,
        csrf_token,
        csrf_scope,
    })
}

pub(crate) async fn export_db(
    State(state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    session: Session,
    Form(form): Form<CsrfTokenForm>,
) -> Result<(StatusCode, HeaderMap, Vec<u8>), MaremmaError> {
    if claims.is_none() {
        // TODO: check that the user is an admin
        return Err(MaremmaError::Unauthorized);
    }

    check_csrf_token(
        &form.csrf_token,
        &form.csrf_scope,
        &[tools_scope()],
        &session,
    )
    .await?;

    let db_filename = state.configuration.read().await.database_file.clone();

    let file_contents = tokio::fs::read(&db_filename)
        .await
        .map_err(MaremmaError::from)?;

    let filename = db_filename.split("/").last().unwrap_or("db.sqlite3");

    let mut headers = HeaderMap::new();

    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(MaremmaError::from)?,
    );

    Ok((StatusCode::OK, headers, file_contents))
}

#[cfg(test)]
/// Use this when you want to be "authenticated"
pub(crate) fn test_user_claims() -> OidcClaims<EmptyAdditionalClaims> {
    OidcClaims::<EmptyAdditionalClaims>(openidconnect::IdTokenClaims::new(
        IssuerUrl::from_url(Url::from_str("https://example.com").expect("Failed to parse URL")),
        vec![],
        chrono::Utc::now() + chrono::Duration::hours(1),
        chrono::Utc::now(),
        StandardClaims::new(SubjectIdentifier::new("testuser@example.com".to_string())),
        EmptyAdditionalClaims {},
    ))
}

#[cfg(test)]
mod tests {

    use crate::db::tests::test_setup;
    use sea_orm::{ColumnTrait, QueryFilter};
    use serde_json::json;
    use std::io::Write;
    use tempfile::NamedTempFile;

    use super::*;

    async fn tools_form(session: &Session, action: FormAction) -> ToolsForm {
        let csrf_scope = tools_scope().to_string();
        let csrf_token = issue_csrf_token(session, &csrf_scope)
            .await
            .expect("Failed to issue CSRF token");
        ToolsForm {
            action: Some(action),
            csrf_token: Some(csrf_token),
            csrf_scope: Some(csrf_scope),
        }
    }

    async fn export_form(session: &Session) -> CsrfTokenForm {
        let csrf_scope = tools_scope().to_string();
        let csrf_token = issue_csrf_token(session, &csrf_scope)
            .await
            .expect("Failed to issue export token");
        CsrfTokenForm {
            csrf_token,
            csrf_scope,
        }
    }

    #[tokio::test]
    async fn test_tools_noauth() {
        use super::*;
        let state = WebState::test().await;

        let res = super::tools(
            State(state.clone()),
            None,
            Query(ToolsQuery::default()),
            state.get_session(),
            Form(ToolsForm {
                action: None,
                csrf_token: None,
                csrf_scope: None,
            }),
        )
        .await;

        assert!(res.is_err());
        assert_eq!(res.into_response().status(), StatusCode::UNAUTHORIZED)
    }

    #[tokio::test]
    async fn test_tools_auth() {
        use super::*;
        let state = WebState::test().await;

        let res = super::tools(
            State(state.clone()),
            Some(test_user_claims()),
            Query(ToolsQuery::default()),
            state.get_session(),
            Form(ToolsForm {
                action: None,
                csrf_token: None,
                csrf_scope: None,
            }),
        )
        .await;

        assert_eq!(res.into_response().status(), StatusCode::OK)
    }
    #[tokio::test]
    async fn test_tools_auth_setallurgent() {
        use super::*;
        let _ = test_setup().await.expect("Failed to start test harness");
        let state = WebState::test().await;

        let session = state.get_session();

        let res = super::tools(
            State(state.clone()),
            Some(test_user_claims()),
            Query(ToolsQuery::default()),
            session.clone(),
            Form(tools_form(&session, FormAction::SetAllToUrgent).await),
        )
        .await
        .into_response();

        dbg!(&res);
        assert_eq!(res.status(), StatusCode::SEE_OTHER)
    }

    #[test]
    fn test_actionstatus_display() {
        use super::ActionStatus;
        assert_eq!(ActionStatus::Success.to_string(), "success");
        assert_eq!(ActionStatus::Error.to_string(), "error");
        assert_eq!(ActionStatus::Unknown.to_string(), "unknown");
    }

    #[tokio::test]
    async fn test_tools_reload_config() {
        test_setup().await.expect("Failed to start test harness");
        use super::*;

        let state = WebState::test().await;
        assert!(tools_reload_config(&state).await.is_err());

        // test reading an invalid file
        let mut state = WebState::test().await;
        let mut tempfile = NamedTempFile::new().expect("Failed to create tempfile");
        tempfile
            .write_all(&[0x01])
            .expect("Failed to write a byte to the tempfile");
        state.config_filepath = tempfile.path().to_path_buf();

        assert!(tools_reload_config(&state).await.is_err());

        // test a valid reload
        let mut state = WebState::test().await;
        let config = Configuration::load_test_config_bare().await;
        let mut tempfile = NamedTempFile::new().expect("Failed to create tempfile");
        tempfile
            .write_all(
                serde_json::to_string(&config)
                    .expect("Failed to serialize test config")
                    .as_bytes(),
            )
            .expect("Failed to write test config");
        state.config_filepath = tempfile.path().to_path_buf();

        assert!(tools_reload_config(&state).await.is_ok());
    }

    #[tokio::test]
    async fn test_tools_reload_config_updates_service_command_line() {
        let mut state = WebState::test().await;
        let mut config = Configuration::load_test_config_bare().await;

        config
            .services
            .get_mut("local_lslah")
            .expect("Failed to find local_lslah in test config")
            .extra_config
            .insert("command_line".to_string(), json!("echo updated"));

        let mut tempfile = NamedTempFile::new().expect("Failed to create tempfile");
        tempfile
            .write_all(
                serde_json::to_string(&config)
                    .expect("Failed to serialize updated test config")
                    .as_bytes(),
            )
            .expect("Failed to write updated test config");
        state.config_filepath = tempfile.path().to_path_buf();

        let res = tools_reload_config(&state).await;
        assert!(res.is_ok());

        let service = entities::service::Entity::find()
            .filter(entities::service::Column::Name.eq("local_lslah"))
            .one(state.db())
            .await
            .expect("Failed to query updated service")
            .expect("Failed to find updated service");
        assert_eq!(service.extra_config["command_line"], json!("echo updated"));

        let service_check = entities::service_check::Entity::find()
            .filter(entities::service_check::Column::ServiceId.eq(service.id))
            .one(state.db())
            .await
            .expect("Failed to query service check")
            .expect("Failed to find service check");

        let rendered = crate::web::views::service_check::service_check_get(
            Path(service_check.id),
            State(state.clone()),
            state.get_session(),
            Some(test_user_claims()),
        )
        .await
        .expect("Failed to render service check after reload")
        .to_string();
        assert!(rendered.contains("echo updated"));
    }

    #[tokio::test]
    async fn test_tools_db_export_invalid_token() {
        test_setup().await.expect("Failed to start test harness");

        let state = WebState::test().await;
        let session = state.get_session();
        assert!(export_db(
            State(state.clone()),
            None,
            session.clone(),
            Form(CsrfTokenForm {
                csrf_token: "lol".to_string(),
                csrf_scope: tools_scope().to_string(),
            }),
        )
        .await
        .is_err());

        let state = WebState::test().await;
        let session = state.get_session();
        let res = export_db(
            State(state.clone()),
            Some(test_user_claims()),
            session.clone(),
            Form(CsrfTokenForm {
                csrf_token: "lol".to_string(),
                csrf_scope: tools_scope().to_string(),
            }),
        )
        .await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_tools_db_export_ok_token() {
        test_setup().await.expect("Failed to start test harness");

        // valid request, session etc
        let (tempfile, state) = WebState::test_with_real_db().await;
        let session = state.get_session();
        let valid_form = export_form(&session).await;

        let res = export_db(
            State(state.clone()),
            Some(test_user_claims()),
            session.clone(),
            Form(valid_form.clone()),
        )
        .await;
        dbg!("result of should-work test", &res);
        assert!(res.is_ok());

        let repeat_res = export_db(
            State(state.clone()),
            Some(test_user_claims()),
            session.clone(),
            Form(valid_form),
        )
        .await;
        assert!(repeat_res.is_ok());

        let res = export_db(
            State(state.clone()),
            Some(test_user_claims()),
            session,
            Form(CsrfTokenForm {
                csrf_token: "definitelynotit".to_string(),
                csrf_scope: tools_scope().to_string(),
            }),
        )
        .await;
        assert!(res.is_err());

        drop(tempfile);
    }
}
