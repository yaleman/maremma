use askama_web::WebTemplate;
use axum::Form;
use chrono::{DateTime, Local, Utc};
use sea_orm::{ColumnTrait, ModelTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::constants::{DEFAULT_SERVICE_CHECK_HISTORY_VIEW_ENTRIES, SESSION_CSRF_TOKEN};
use crate::web::views::csrf::{check_csrf_token, issue_csrf_token, CsrfRedirectToForm};
use crate::web::MaremmaError;

use super::prelude::*;
use crate::web::views::tools::ActionStatus;

#[derive(Template, Debug, WebTemplate)]
#[template(path = "service_check.html")]
pub(crate) struct ServiceCheckTemplate {
    title: String,
    username: Option<String>, // for the header
    message: Option<String>,
    status: ActionStatus,
    service_check: entities::service_check::Model,
    host: entities::host::Model,
    service: entities::service::Model,
    service_check_history: Vec<entities::service_check_history::Model>,
    parsed_config: Option<String>,
    last_check_display: String,
    last_check_relative: String,
    next_check_display: String,
    next_check_relative: String,
    csrf_token: String,
}

fn format_absolute_time(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M:%S %Z")
        .to_string()
}

fn format_relative_time(value: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let delta = value - now;
    let future = delta > chrono::Duration::zero();
    let seconds = delta.num_seconds().abs();

    let (count, unit) = if seconds < 60 {
        (seconds, "second")
    } else if seconds < 3_600 {
        (seconds / 60, "minute")
    } else if seconds < 86_400 {
        (seconds / 3_600, "hour")
    } else {
        (seconds / 86_400, "day")
    };

    let suffix = if count == 1 { "" } else { "s" };

    if count == 0 {
        if future {
            "now".to_string()
        } else {
            "just now".to_string()
        }
    } else if future {
        format!("in {count} {unit}{suffix}")
    } else {
        format!("{count} {unit}{suffix} ago")
    }
}

fn format_time_fields(value: DateTime<Utc>, now: DateTime<Utc>) -> (String, String) {
    (
        format_absolute_time(value),
        format_relative_time(value, now),
    )
}

pub(crate) async fn service_check_get(
    Path(service_check_id): Path<Uuid>,
    State(state): State<WebState>,
    session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<ServiceCheckTemplate, MaremmaError> {
    let user = check_login(claims)?;

    let db_lock = state.db();

    let res = entities::service_check::Entity::find_by_id(service_check_id)
        .one(db_lock)
        .await
        .map_err(|err| {
            error!(
                "Failed to search for service check {}: {:?}",
                service_check_id, err
            );
            MaremmaError::ServiceCheckNotFound(service_check_id)
        })?;
    let service_check = res.ok_or(MaremmaError::ServiceCheckNotFound(service_check_id))?;

    let service_check_history = entities::service_check_history::Entity::find()
        .filter(entities::service_check_history::Column::ServiceCheckId.eq(service_check_id))
        .order_by_desc(entities::service_check_history::Column::Timestamp)
        .limit(DEFAULT_SERVICE_CHECK_HISTORY_VIEW_ENTRIES)
        .all(db_lock)
        .await
        .map_err(|err| {
            error!(
                "Failed to search for service check history {}: {:?}",
                service_check_id, err
            );
            MaremmaError::from(err)
        })?;

    let host = service_check
        .find_related(entities::host::Entity)
        .one(db_lock)
        .await
        .map_err(|err| {
            error!(
                "Failed to search for service check {}: {:?}",
                service_check.id, err
            );
            MaremmaError::from(err)
        })?
        .ok_or_else(|| {
            error!(
                "Host not found in DB for service check id {}",
                service_check_id
            );
            MaremmaError::ServiceCheckNotFound(service_check_id)
        })?;

    let service = service_check
        .find_related(entities::service::Entity)
        .one(db_lock)
        .await
        .map_err(|err| {
            error!(
                "Error querying service for service_check={} error={}",
                service_check_id, err
            );
            MaremmaError::from(err)
        })?
        .ok_or_else(|| {
            error!(
                "Service not found in DB for service check id {}",
                service_check_id
            );
            MaremmaError::ServiceCheckNotFound(service_check_id)
        })?;

    let mut parsed_service = crate::services::Service::try_from_service_model(&service, db_lock)
        .await
        .map_err(|err| {
            error!(
                "Failed to render service_check {} into service {:?}",
                service_check_id, err
            );
            MaremmaError::Configuration("Failed to parse service definition".to_string())
        })?;

    parsed_service.parse_config().map_err(|err| {
        error!(
            "Failed to render service_check {} into service {:?}",
            service_check_id, err
        );
        MaremmaError::Configuration("Failed to parse service definition to config".to_string())
    })?;

    let parsed_config = parsed_service.config().map(|liveservice| {
        let res = liveservice
            .as_json_pretty(&host)
            .map_err(|err| {
                error!(
                    "Failed to render service_check {} into service {:?}",
                    service_check_id, err
                );
                MaremmaError::Configuration("Failed to overlay host config".to_string())
            })
            .unwrap_or("Failed to render config".to_string());
        debug!("Parsed config: {}", res);
        res
    });

    let now = Utc::now();
    let (last_check_display, last_check_relative) =
        format_time_fields(service_check.last_check, now);
    let (next_check_display, next_check_relative) =
        format_time_fields(service_check.next_check, now);
    let csrf_token = issue_csrf_token(&state, &session).await?;

    Ok(ServiceCheckTemplate {
        title: format!("Service Check: {}", &service.name),
        username: Some(user.username()),
        message: None,
        status: ActionStatus::Unknown,
        service_check,
        host,
        service,
        service_check_history,
        parsed_config,
        last_check_display,
        last_check_relative,
        next_check_display,
        next_check_relative,
        csrf_token,
    })
}

pub(crate) async fn set_service_check_urgent(
    Path(service_check_id): Path<Uuid>,
    State(state): State<WebState>,
    session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    Form(form): Form<CsrfRedirectToForm>,
) -> Result<Redirect, impl IntoResponse> {
    set_service_check_status(
        service_check_id,
        state,
        session,
        claims,
        ServiceStatus::Urgent,
        form,
    )
    .await
}
pub(crate) async fn set_service_check_disabled(
    Path(service_check_id): Path<Uuid>,
    State(state): State<WebState>,
    session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    Form(form): Form<CsrfRedirectToForm>,
) -> Result<Redirect, impl IntoResponse> {
    set_service_check_status(
        service_check_id,
        state,
        session,
        claims,
        ServiceStatus::Disabled,
        form,
    )
    .await
}

pub(crate) async fn set_service_check_enabled(
    Path(service_check_id): Path<Uuid>,
    State(state): State<WebState>,
    session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    Form(form): Form<CsrfRedirectToForm>,
) -> Result<Redirect, impl IntoResponse> {
    set_service_check_status(
        service_check_id,
        state,
        session,
        claims,
        ServiceStatus::Pending,
        form,
    )
    .await
}

pub(crate) async fn set_service_check_status(
    service_check_id: Uuid,
    state: WebState,
    session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    status: ServiceStatus,
    form: CsrfRedirectToForm,
) -> Result<Redirect, MaremmaError> {
    let _user = check_login(claims)?;
    check_csrf_token(&form.csrf_token, &session).await?;

    let db_lock = state.db();
    let service_check = entities::service_check::Entity::find_by_id(service_check_id)
        .one(db_lock)
        .await
        .map_err(|err| {
            error!(
                "Failed to search for service check {}: {:?}",
                service_check_id, err
            );
            MaremmaError::from(err)
        })?;

    let service_check = match service_check {
        Some(service_check) => service_check,
        None => return Err(MaremmaError::ServiceCheckNotFound(service_check_id)),
    };

    let mut service_check = service_check.into_active_model();
    service_check.status.set_if_not_equals(status);
    service_check
        .last_updated
        .set_if_not_equals(chrono::Utc::now());

    let host_id = service_check.host_id.clone().unwrap();

    if service_check.is_changed() {
        service_check.save(db_lock).await.map_err(|err| {
            error!(
                "Failed to set service_check_id={} to status={}: {:?}",
                service_check_id, status, err
            );
            MaremmaError::from(err)
        })?;
    };

    // TODO: make it so we can redirect to... elsewhere based on a query string?
    if let Some(redirect_to) = &form.redirect_to {
        Ok(Redirect::to(redirect_to))
    } else {
        Ok(Redirect::to(&format!(
            "{}/{}",
            Urls::Host,
            host_id.hyphenated()
        )))
    }
}

/// Want to delete a service check? Woo!
pub(crate) async fn service_check_delete(
    Path(service_check_id): Path<Uuid>,
    State(state): State<WebState>,
    session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    Form(redirect_form): Form<CsrfRedirectToForm>,
) -> Result<Redirect, MaremmaError> {
    let _user = check_login(claims)?;
    check_csrf_token(&redirect_form.csrf_token, &session).await?;
    let db_lock = state.db();
    entities::service_check::Entity::delete_by_id(service_check_id)
        .exec(db_lock)
        .await
        .map_err(|err| {
            error!(
                "Failed to delete service check {}: {:?}",
                service_check_id, err
            );
            MaremmaError::from(err)
        })?;

    if let Some(redirect_to) = redirect_form.redirect_to {
        Ok(Redirect::to(&redirect_to))
    } else {
        Ok(Redirect::to(Urls::Index.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use crate::db::tests::test_setup;
    use crate::web::views::tools::test_user_claims;
    use std::path::PathBuf;

    use super::*;

    async fn csrf_form(
        state: &WebState,
        session: &Session,
        redirect_to: Option<String>,
    ) -> CsrfRedirectToForm {
        let csrf_token = issue_csrf_token(state, session)
            .await
            .expect("Failed to issue CSRF token");
        CsrfRedirectToForm {
            redirect_to,
            csrf_token,
        }
    }

    #[tokio::test]
    async fn test_view_service_check_without_auth() {
        let state = WebState::test().await;

        let service_check = entities::service_check::Entity::find()
            .one(state.db())
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");
        let res = service_check_get(
            Path(service_check.id),
            State(state.clone()),
            state.get_session(),
            None,
        )
        .await;

        assert!(res.is_err()); // because authentication failed
    }

    #[tokio::test]
    async fn test_view_service_check_with_auth() {
        let state = WebState::test().await;

        let service_check = entities::service_check::Entity::find()
            .one(state.db())
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = service_check_get(
            Path(service_check.id),
            State(state.clone()),
            state.get_session(),
            Some(test_user_claims()),
        )
        .await
        .expect("Failed to auth!");

        let res = res.to_string();

        dbg!(&res);

        assert!(res.contains("Service Check"))
    }

    #[test]
    fn test_format_relative_time() {
        let now = Utc.with_ymd_and_hms(2026, 3, 23, 10, 0, 0).unwrap();

        assert_eq!(
            format_relative_time(Utc.with_ymd_and_hms(2026, 3, 23, 10, 0, 30).unwrap(), now),
            "in 30 seconds"
        );
        assert_eq!(
            format_relative_time(Utc.with_ymd_and_hms(2026, 3, 23, 9, 55, 0).unwrap(), now),
            "5 minutes ago"
        );
        assert_eq!(
            format_relative_time(Utc.with_ymd_and_hms(2026, 3, 23, 12, 0, 0).unwrap(), now),
            "in 2 hours"
        );
    }

    #[tokio::test]
    async fn test_set_service_check_urgent() {
        let (db, config) = test_setup().await.expect("Failed to set up!");

        let state = WebState::new(db, config, None, None, PathBuf::new());
        let session = state.get_session();

        let service_check = entities::service_check::Entity::find()
            .one(state.db())
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = set_service_check_urgent(
            Path(service_check.id),
            State(state.clone()),
            session.clone(),
            Some(test_user_claims()),
            Form(csrf_form(&state, &session, None).await),
        )
        .await;
        assert!(res.is_ok());
        let res = set_service_check_urgent(
            Path(Uuid::new_v4()),
            State(state.clone()),
            session.clone(),
            Some(test_user_claims()),
            Form(csrf_form(&state, &session, Some("/test".to_string())).await),
        )
        .await;
        assert!(res.is_err());

        let res = set_service_check_urgent(
            Path(Uuid::new_v4()),
            State(state.clone()),
            session.clone(),
            Some(test_user_claims()),
            Form(csrf_form(&state, &session, Some("/test".to_string())).await),
        )
        .await;

        assert!(res.is_err());
    }
    #[tokio::test]
    async fn test_set_service_check_disabled() {
        let state = WebState::test().await;
        let session = state.get_session();

        let service_check = entities::service_check::Entity::find()
            .one(state.db())
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = set_service_check_disabled(
            Path(service_check.id),
            State(state.clone()),
            session.clone(),
            Some(test_user_claims()),
            Form(csrf_form(&state, &session, None).await),
        )
        .await;
        assert!(res.is_ok());
        let res = set_service_check_disabled(
            Path(Uuid::new_v4()),
            State(state.clone()),
            session.clone(),
            Some(test_user_claims()),
            Form(csrf_form(&state, &session, None).await),
        )
        .await;
        assert!(res.is_err());
    }
    #[tokio::test]
    async fn test_set_service_check_enabled() {
        let (db, config) = test_setup().await.expect("Failed to set up!");

        let state = WebState::new(db, config, None, None, PathBuf::new());
        let session = state.get_session();

        let service_check = entities::service_check::Entity::find()
            .one(state.db())
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = set_service_check_enabled(
            Path(service_check.id),
            State(state.clone()),
            session.clone(),
            Some(test_user_claims()),
            Form(csrf_form(&state, &session, None).await),
        )
        .await;
        assert!(res.is_ok());
        let res = set_service_check_enabled(
            Path(Uuid::new_v4()),
            State(state.clone()),
            session.clone(),
            Some(test_user_claims()),
            Form(csrf_form(&state, &session, None).await),
        )
        .await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_view_missing_service_check_with_auth() {
        use super::*;
        let state = WebState::test().await;

        let mut service_check_id = Uuid::new_v4();
        while entities::service_check::Entity::find_by_id(service_check_id)
            .one(state.db())
            .await
            .expect("Failed to search for service_check")
            .is_some()
        {
            service_check_id = Uuid::new_v4();
        }
        let res = super::service_check_get(
            Path(service_check_id),
            State(state.clone()),
            state.get_session(),
            Some(test_user_claims()),
        )
        .await;

        dbg!(&res);
        assert!(res.is_err());
        assert_eq!(res.into_response().status(), StatusCode::NOT_FOUND)
    }

    #[tokio::test]
    async fn test_view_service_check_delete_unauth() {
        use super::*;
        let (_db, _config) = test_setup().await.expect("Failed to set up!");

        let state = WebState::test().await;
        let session = state.get_session();

        let mut service_check_id = Uuid::new_v4();
        while entities::service_check::Entity::find_by_id(service_check_id)
            .one(state.db())
            .await
            .expect("Failed to search for service_check")
            .is_some()
        {
            service_check_id = Uuid::new_v4();
        }
        let res = super::service_check_delete(
            Path(service_check_id),
            State(state.clone()),
            session.clone(),
            None,
            Form(csrf_form(&state, &session, None).await),
        )
        .await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_view_service_check_delete_auth() {
        use super::*;
        let (db, _config) = test_setup().await.expect("Failed to set up!");

        let state = WebState::test().await;
        let session = state.get_session();

        let mut service_check_id = Uuid::new_v4();
        while entities::service_check::Entity::find_by_id(service_check_id)
            .one(state.db())
            .await
            .expect("Failed to search for service_check")
            .is_some()
        {
            service_check_id = Uuid::new_v4();
        }
        let res = super::service_check_delete(
            Path(service_check_id),
            State(state.clone()),
            session.clone(),
            Some(test_user_claims()),
            Form(csrf_form(&state, &session, None).await),
        )
        .await;

        dbg!(&res);
        assert!(res.is_ok());
        assert_eq!(res.into_response().status(), StatusCode::SEE_OTHER);

        // find a valid service check
        let service_check = entities::service_check::Entity::find()
            .one(db.as_ref())
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = service_check_delete(
            Path(service_check.id),
            State(state.clone()),
            session.clone(),
            Some(test_user_claims()),
            Form(csrf_form(&state, &session, None).await),
        )
        .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_service_check_actions_require_valid_csrf() {
        let state = WebState::test().await;
        let session = state.get_session();
        let service_check = entities::service_check::Entity::find()
            .one(state.db())
            .await
            .expect("Failed to query service check")
            .expect("No service checks found");

        let res = set_service_check_urgent(
            Path(service_check.id),
            State(state.clone()),
            session.clone(),
            Some(test_user_claims()),
            Form(CsrfRedirectToForm {
                redirect_to: None,
                csrf_token: "wrong".to_string(),
            }),
        )
        .await;
        assert!(res.is_err());
        assert_eq!(
            res.expect_err("Expected csrf error")
                .into_response()
                .status(),
            StatusCode::FORBIDDEN
        );

        let csrf_token = issue_csrf_token(&state, &session)
            .await
            .expect("Failed to issue CSRF token");
        let res = set_service_check_urgent(
            Path(service_check.id),
            State(state.clone()),
            session.clone(),
            Some(test_user_claims()),
            Form(CsrfRedirectToForm {
                redirect_to: None,
                csrf_token: format!("{csrf_token}-wrong"),
            }),
        )
        .await;
        assert!(res.is_err());
        assert_eq!(
            res.expect_err("Expected csrf mismatch")
                .into_response()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}
