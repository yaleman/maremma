use super::prelude::*;
use crate::db::update_db_from_config;
use crate::web::Configuration;
use axum::Form;
use sea_orm::prelude::Expr;
use tokio::sync::RwLock;

#[cfg(test)]
use openidconnect::{IssuerUrl, StandardClaims, SubjectIdentifier};
#[cfg(test)]
use reqwest::Url;
#[cfg(test)]
use std::str::FromStr;

#[derive(Template, Debug)]
#[template(path = "tools.html")]
pub(crate) struct ToolsTemplate {
    title: String,
    username: Option<String>,
    message: Option<String>,
    status: ActionStatus,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FormAction {
    SetAllToUrgent,
    ReloadConfig,
}

#[derive(Deserialize, Default, Debug)]
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
            ActionStatus::Error => write!(f, "danger"),
            ActionStatus::Unknown => write!(f, "primary"),
        }
    }
}

#[derive(Deserialize, Default)]
pub(crate) struct ToolsForm {
    action: Option<FormAction>,
}
#[derive(Deserialize, Default)]
pub(crate) struct ToolsQuery {
    result: Option<String>,
    #[serde(default)]
    status: ActionStatus,
}

#[instrument(level = "info", skip_all)]
async fn tools_reload_config(state: WebState) -> Result<(), Redirect> {
    info!("Asked to reload config");

    let new_config = Configuration::new(&state.config_filepath)
        .await
        .map_err(|e| {
            error!("Failed to reload config: {:?}", e);
            Redirect::to(&format!(
                "{}?result=Failed to load config from file&status={}",
                Urls::Tools,
                ActionStatus::Error,
            ))
        })?;

    *state.configuration.write().await = new_config;

    let new_config = Configuration::new(&state.config_filepath)
        .await
        .map_err(|e| {
            error!("Failed to reload config: {:?}", e);
            Redirect::to(&format!(
                "{}?result=Failed to load config from file&status={}",
                Urls::Tools,
                ActionStatus::Error,
            ))
        })?;
    update_db_from_config(state.db.as_ref(), Arc::new(RwLock::new(new_config)))
        .await
        .map_err(|e| {
            error!("Failed to reload config: {:?}", e);
            Redirect::to(&format!(
                "{}?result=Failed to reload config&status={}",
                Urls::Tools,
                ActionStatus::Error,
            ))
        })?;

    info!("Reloaded config");
    // not really an error but we're doing this to show the user that the config was reloaded
    Err(Redirect::to(&format!(
        "{}?result=Reloaded config&status={}",
        Urls::Tools,
        ActionStatus::Success,
    )))
}

/// Seen at `/tools`
pub(crate) async fn tools(
    State(state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    Query(results): Query<ToolsQuery>,
    Form(form): Form<ToolsForm>,
) -> Result<ToolsTemplate, impl IntoResponse> {
    if claims.is_none() {
        // TODO: check that the user is an admin
        return Err(StatusCode::UNAUTHORIZED.into_response());
    }

    if let Some(action) = form.action {
        match action {
            FormAction::SetAllToUrgent => {
                info!("Asked to set all to urgent");
                entities::service_check::Entity::update_many()
                    .col_expr(
                        entities::service_check::Column::Status,
                        Expr::value(ServiceStatus::Urgent),
                    )
                    .exec(state.db.as_ref())
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
                return Err(Redirect::to(&format!(
                    "{}?result=Set all tasks to urgent&status={}",
                    Urls::Tools,
                    ActionStatus::Success,
                ))
                .into_response());
            }
            FormAction::ReloadConfig => {
                if let Err(err) = tools_reload_config(state).await {
                    return Err(err.into_response());
                };
            }
        }
    }

    Ok(ToolsTemplate {
        title: "Tools".to_string(),
        username: claims.map(|c: OidcClaims<EmptyAdditionalClaims>| User::from(c).username()),
        message: results.result,
        status: results.status,
    })
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

    use std::io::Write;
    use std::path::PathBuf;

    use tempfile::NamedTempFile;

    use crate::db::tests::test_setup;
    // use std::path::PathBuf;

    #[tokio::test]
    async fn test_tools_noauth() {
        use super::*;
        let state = WebState::test().await;

        let res = super::tools(
            State(state.clone()),
            None,
            Query(ToolsQuery::default()),
            Form(ToolsForm::default()),
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
            Form(ToolsForm::default()),
        )
        .await;

        assert_eq!(res.into_response().status(), StatusCode::OK)
    }
    #[tokio::test]
    async fn test_tools_auth_setallurgent() {
        use super::*;
        let state = WebState::test().await;

        let res = super::tools(
            State(state.clone()),
            Some(test_user_claims()),
            Query(ToolsQuery::default()),
            Form(ToolsForm {
                action: Some(FormAction::SetAllToUrgent),
            }),
        )
        .await;

        assert_eq!(res.into_response().status(), StatusCode::SEE_OTHER)
    }

    #[test]
    fn test_actionstatus_display() {
        use super::ActionStatus;
        assert_eq!(ActionStatus::Success.to_string(), "success");
        assert_eq!(ActionStatus::Error.to_string(), "danger");
        assert_eq!(ActionStatus::Unknown.to_string(), "primary");
    }

    #[tokio::test]
    async fn test_tools_reload_config() {
        test_setup().await.expect("Failed to start test harness");
        use super::*;

        let state = WebState::test().await;
        let res = tools_reload_config(state.clone()).await;
        assert!(res.is_err());
        dbg!(&res);

        if let Err(err) = res {
            let err = err.into_response();
            assert_eq!(err.status(), StatusCode::SEE_OTHER);
            let (headers, _body) = err.into_parts();
            assert_eq!(
                headers
                    .headers
                    .get("location")
                    .expect("Failed to get location header")
                    .to_str()
                    .expect("Failed to get location header value"),
                &format!(
                    "{}?result=Failed to load config from file&status={}",
                    Urls::Tools,
                    ActionStatus::Error,
                ),
                "Expected an error response"
            );
        }

        // test reading an invalid file
        let mut state = WebState::test().await;
        let mut tempfile = NamedTempFile::new().expect("Failed to create tempfile");
        tempfile
            .write_all(&[0x01])
            .expect("Failed to write a byte to the tempfile");
        state.config_filepath = tempfile.path().to_path_buf();

        let res = tools_reload_config(state.clone()).await;
        if let Err(err) = res {
            let err = err.into_response();
            assert_eq!(err.status(), StatusCode::SEE_OTHER);
            let (headers, _body) = err.into_parts();
            assert_eq!(
                headers
                    .headers
                    .get("location")
                    .expect("Failed to get location header")
                    .to_str()
                    .expect("Failed to get location header value"),
                &format!(
                    "{}?result=Failed to load config from file&status={}",
                    Urls::Tools,
                    ActionStatus::Error,
                ),
                "Expected a failed reload"
            );
        }

        // test a valid reload
        let mut state = WebState::test().await;
        state.config_filepath =
            PathBuf::from_str("maremma.example.json").expect("failed to pathbuf test config");

        let res = tools_reload_config(state.clone()).await;
        if let Err(err) = res {
            let err = err.into_response();
            assert_eq!(err.status(), StatusCode::SEE_OTHER);
            let (headers, _body) = err.into_parts();
            assert_eq!(
                headers
                    .headers
                    .get("location")
                    .expect("Failed to get location header")
                    .to_str()
                    .expect("Failed to get location header value"),
                &format!(
                    "{}?result=Reloaded config&status={}",
                    Urls::Tools,
                    ActionStatus::Success
                ),
                "Expected a failed reload"
            );
        }
    }
}
