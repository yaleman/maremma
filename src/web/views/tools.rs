use axum::Form;
use sea_orm::prelude::Expr;

use super::prelude::*;

#[derive(Template)]
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
}

#[derive(Deserialize, Default)]
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

#[derive(Deserialize)]
pub(crate) struct ToolsForm {
    action: Option<FormAction>,
}
#[derive(Deserialize)]
pub(crate) struct ToolsQuery {
    result: Option<String>,
    #[serde(default)]
    status: ActionStatus,
}

/// Seen at `/tools`
pub(crate) async fn tools(
    State(state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    Query(results): Query<ToolsQuery>,
    Form(form): Form<ToolsForm>,
) -> Result<ToolsTemplate, impl IntoResponse> {
    if claims.is_none() {
        // TODO: admin checks
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
                        Redirect::to("/tools?result=Failed to set all tasks to urgent&status=error")
                            .into_response()
                    })?;
                return Err(
                    Redirect::to("/tools?result=Set all tasks to urgent&status=success")
                        .into_response(),
                );
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
use openidconnect::{IssuerUrl, StandardClaims, SubjectIdentifier};
#[cfg(test)]
use reqwest::Url;
#[cfg(test)]
use std::str::FromStr;

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
