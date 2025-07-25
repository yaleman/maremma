use askama_web::WebTemplate;
use axum::Form;
use sea_orm::{ColumnTrait, ModelTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::constants::DEFAULT_SERVICE_CHECK_HISTORY_VIEW_ENTRIES;
use crate::web::Error;

use super::prelude::*;

#[derive(Template, Debug, WebTemplate)]
#[template(path = "service_check.html")]
pub(crate) struct ServiceCheckTemplate {
    title: String,
    username: Option<String>, // for the header
    message: Option<String>,
    status: String,
    service_check: entities::service_check::Model,
    host: entities::host::Model,
    service: entities::service::Model,
    service_check_history: Vec<entities::service_check_history::Model>,
    parsed_config: Option<String>,
}

pub(crate) async fn service_check_get(
    Path(service_check_id): Path<Uuid>,
    State(state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<ServiceCheckTemplate, (StatusCode, String)> {
    let user = check_login(claims)?;

    let db_lock = state.get_db_lock().await;

    let res = entities::service_check::Entity::find_by_id(service_check_id)
        .one(&*db_lock)
        .await
        .map_err(|err| {
            error!(
                "Failed to search for service check {}: {:?}",
                service_check_id, err
            );
            (
                StatusCode::NOT_FOUND,
                format!("Service check with id={service_check_id} not found"),
            )
        })?;
    let service_check = res.ok_or((
        StatusCode::NOT_FOUND,
        format!("Service check with id={service_check_id} not found"),
    ))?;

    let service_check_history = entities::service_check_history::Entity::find()
        .filter(entities::service_check_history::Column::ServiceCheckId.eq(service_check_id))
        .order_by_desc(entities::service_check_history::Column::Timestamp)
        .limit(DEFAULT_SERVICE_CHECK_HISTORY_VIEW_ENTRIES)
        .all(&*db_lock)
        .await
        .map_err(|err| {
            error!(
                "Failed to search for service check history {}: {:?}",
                service_check_id, err
            );
            Error::from(err)
        })?;

    let host = service_check
        .find_related(entities::host::Entity)
        .one(&*db_lock)
        .await
        .map_err(|err| {
            error!(
                "Failed to search for service check {}: {:?}",
                service_check.id, err
            );
            Error::from(err)
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Service check with id={service_check_id} host not found"),
            )
        })?;

    let service = service_check
        .find_related(entities::service::Entity)
        .one(&*db_lock)
        .await
        .map_err(|err| {
            error!(
                "Error querying service for service_check={} error={}",
                service_check_id, err
            );
            Error::from(err)
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!(
                    "Service check with id={service_check_id} service not found"
                ),
            )
        })?;

    let mut parsed_service = crate::services::Service::try_from_service_model(&service, &db_lock)
        .await
        .map_err(|err| {
            error!(
                "Failed to render service_check {} into service {:?}",
                service_check_id, err
            );
            Error::Configuration("Failed to parse service definition".to_string())
        })?;
    drop(db_lock);

    parsed_service.parse_config().map_err(|err| {
        error!(
            "Failed to render service_check {} into service {:?}",
            service_check_id, err
        );
        Error::Configuration("Failed to parse service definition to config".to_string())
    })?;

    let parsed_config = parsed_service.config().map(|liveservice| {
        let res = liveservice
            .as_json_pretty(&host)
            .map_err(|err| {
                error!(
                    "Failed to render service_check {} into service {:?}",
                    service_check_id, err
                );
                Error::Configuration("Failed to overlay host config".to_string())
            })
            .unwrap_or("Failed to render config".to_string());
        debug!("Parsed config: {}", res);
        res
    });

    Ok(ServiceCheckTemplate {
        title: format!("Service Check: {}", &service.name),
        username: Some(user.username()),
        message: None,
        status: "".to_string(),
        service_check,
        host,
        service,
        service_check_history,
        parsed_config,
    })
}

pub(crate) async fn set_service_check_urgent(
    Path(service_check_id): Path<Uuid>,
    State(state): State<WebState>,
    Form(form): Form<RedirectTo>,
) -> Result<Redirect, impl IntoResponse> {
    set_service_check_status(service_check_id, state, ServiceStatus::Urgent, form).await
}
pub(crate) async fn set_service_check_disabled(
    Path(service_check_id): Path<Uuid>,
    State(state): State<WebState>,
    Form(form): Form<RedirectTo>,
) -> Result<Redirect, impl IntoResponse> {
    set_service_check_status(service_check_id, state, ServiceStatus::Disabled, form).await
}

pub(crate) async fn set_service_check_enabled(
    Path(service_check_id): Path<Uuid>,
    State(state): State<WebState>,
    Form(form): Form<RedirectTo>,
) -> Result<Redirect, impl IntoResponse> {
    set_service_check_status(service_check_id, state, ServiceStatus::Pending, form).await
}

pub(crate) async fn set_service_check_status(
    service_check_id: Uuid,
    state: WebState,
    status: ServiceStatus,
    form: RedirectTo,
) -> Result<Redirect, (StatusCode, String)> {
    let db_lock = state.get_db_lock().await;
    let service_check = entities::service_check::Entity::find_by_id(service_check_id)
        .one(&*db_lock)
        .await
        .map_err(|err| {
            error!(
                "Failed to search for service check {}: {:?}",
                service_check_id, err
            );
            Error::from(err)
        })?;

    let service_check = match service_check {
        Some(service_check) => service_check,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                format!("Service check with id={service_check_id} not found"),
            ))
        }
    };

    let mut service_check = service_check.into_active_model();
    service_check.status.set_if_not_equals(status);
    service_check
        .last_updated
        .set_if_not_equals(chrono::Utc::now());

    let host_id = service_check.host_id.clone().unwrap();

    if service_check.is_changed() {
        service_check.save(&*db_lock).await.map_err(|err| {
            error!(
                "Failed to set service_check_id={} to status={}: {:?}",
                service_check_id, status, err
            );
            Error::from(err)
        })?;
    };
    drop(db_lock);

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

/// For when you want to redirect people back to where they came from
#[derive(Deserialize, Debug)]
pub(crate) struct RedirectTo {
    redirect_to: Option<String>,
}

impl From<Option<String>> for RedirectTo {
    fn from(redirect_to: Option<String>) -> Self {
        Self { redirect_to }
    }
}

/// Want to delete a service check? Woo!
pub(crate) async fn service_check_delete(
    Path(service_check_id): Path<Uuid>,
    State(state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    Form(redirect_form): Form<RedirectTo>,
) -> Result<Redirect, (StatusCode, String)> {
    let _user = claims.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            "You must be logged in to view this page".to_string(),
        )
    })?;
    let db_lock = state.get_db_lock().await;
    entities::service_check::Entity::delete_by_id(service_check_id)
        .exec(&*db_lock)
        .await
        .map_err(|err| {
            error!(
                "Failed to delete service check {}: {:?}",
                service_check_id, err
            );
            Error::from(err)
        })?;
    drop(db_lock);

    if let Some(redirect_to) = redirect_form.redirect_to {
        Ok(Redirect::to(&redirect_to))
    } else {
        Ok(Redirect::to(Urls::Index.as_ref()))
    }
}

#[cfg(test)]
mod tests {

    use crate::db::tests::test_setup;
    use crate::web::views::tools::test_user_claims;
    use std::path::PathBuf;

    use super::*;

    #[tokio::test]
    async fn test_view_service_check_without_auth() {
        let state = WebState::test().await;

        let service_check = entities::service_check::Entity::find()
            .one(&*state.get_db_lock().await)
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");
        let res = service_check_get(Path(service_check.id), State(state.clone()), None).await;

        assert!(res.is_err()); // because authentication failed
    }

    #[tokio::test]
    async fn test_view_service_check_with_auth() {
        let state = WebState::test().await;

        let service_check = entities::service_check::Entity::find()
            .one(&*state.get_db_lock().await)
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = service_check_get(
            Path(service_check.id),
            State(state.clone()),
            Some(test_user_claims()),
        )
        .await
        .expect("Failed to auth!");

        let res = res.to_string();

        dbg!(&res);

        assert!(res.contains("Service Check"))
    }

    #[tokio::test]
    async fn test_set_service_check_urgent() {
        let (db, config) = test_setup().await.expect("Failed to set up!");

        let state = WebState::new(db, config, None, None, PathBuf::new());

        let service_check = entities::service_check::Entity::find()
            .one(&*state.get_db_lock().await)
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = set_service_check_urgent(
            Path(service_check.id),
            State(state.clone()),
            Form(RedirectTo::from(None)),
        )
        .await;
        assert!(res.is_ok());
        let res = set_service_check_urgent(
            Path(Uuid::new_v4()),
            State(state.clone()),
            Form(RedirectTo {
                redirect_to: Some("/test".to_string()),
            }),
        )
        .await;
        assert!(res.is_err());

        let res = set_service_check_urgent(
            Path(Uuid::new_v4()),
            State(state.clone()),
            Form(RedirectTo {
                redirect_to: Some("/test".to_string()),
            }),
        )
        .await;

        assert!(res.is_err());
    }
    #[tokio::test]
    async fn test_set_service_check_disabled() {
        let state = WebState::test().await;

        let service_check = entities::service_check::Entity::find()
            .one(&*state.get_db_lock().await)
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = set_service_check_disabled(
            Path(service_check.id),
            State(state.clone()),
            Form(RedirectTo::from(None)),
        )
        .await;
        assert!(res.is_ok());
        let res = set_service_check_disabled(
            Path(Uuid::new_v4()),
            State(state),
            Form(RedirectTo::from(None)),
        )
        .await;
        assert!(res.is_err());
    }
    #[tokio::test]
    async fn test_set_service_check_enabled() {
        let (db, config) = test_setup().await.expect("Failed to set up!");

        let state = WebState::new(db, config, None, None, PathBuf::new());

        let service_check = entities::service_check::Entity::find()
            .one(&*state.get_db_lock().await)
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = set_service_check_enabled(
            Path(service_check.id),
            State(state.clone()),
            Form(RedirectTo::from(None)),
        )
        .await;
        assert!(res.is_ok());
        let res = set_service_check_enabled(
            Path(Uuid::new_v4()),
            State(state),
            Form(RedirectTo::from(None)),
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
            .one(&*state.get_db_lock().await)
            .await
            .expect("Failed to search for service_check")
            .is_some()
        {
            service_check_id = Uuid::new_v4();
        }
        let res = super::service_check_get(
            Path(service_check_id),
            State(state.clone()),
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

        let mut service_check_id = Uuid::new_v4();
        while entities::service_check::Entity::find_by_id(service_check_id)
            .one(&*state.get_db_lock().await)
            .await
            .expect("Failed to search for service_check")
            .is_some()
        {
            service_check_id = Uuid::new_v4();
        }
        let res = super::service_check_delete(
            Path(service_check_id),
            State(state.clone()),
            None,
            Form(RedirectTo { redirect_to: None }),
        )
        .await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_view_service_check_delete_auth() {
        use super::*;
        let (db, _config) = test_setup().await.expect("Failed to set up!");

        let state = WebState::test().await;

        let mut service_check_id = Uuid::new_v4();
        while entities::service_check::Entity::find_by_id(service_check_id)
            .one(&*state.get_db_lock().await)
            .await
            .expect("Failed to search for service_check")
            .is_some()
        {
            service_check_id = Uuid::new_v4();
        }
        let res = super::service_check_delete(
            Path(service_check_id),
            State(state.clone()),
            Some(test_user_claims()),
            Form(RedirectTo { redirect_to: None }),
        )
        .await;

        dbg!(&res);
        assert!(res.is_ok());
        assert_eq!(res.into_response().status(), StatusCode::SEE_OTHER);

        // find a valid service check
        let service_check = entities::service_check::Entity::find()
            .one(&*db.write().await)
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = service_check_delete(
            Path(service_check.id),
            State(state.clone()),
            Some(test_user_claims()),
            Form(RedirectTo { redirect_to: None }),
        )
        .await;
        assert!(res.is_ok());
    }
}
