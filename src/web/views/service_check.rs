use sea_orm::{ModelTrait, QuerySelect};

use crate::constants::DEFAULT_SERVICE_CHECK_HISTORY_LIMIT;

use super::prelude::*;

#[derive(Template, Debug)]
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
}

pub(crate) async fn service_check_get(
    Path(service_check_id): Path<Uuid>,
    State(state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<ServiceCheckTemplate, (StatusCode, String)> {
    let user = claims.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            "You must be logged in to view this page".to_string(),
        )
    })?;

    let user: User = user.into();

    let res = entities::service_check::Entity::find_by_id(service_check_id)
        .find_with_related(entities::service_check_history::Entity)
        .limit(DEFAULT_SERVICE_CHECK_HISTORY_LIMIT)
        .all(state.db.as_ref())
        .await
        .map_err(|err| {
            error!(
                "Failed to search for service check {}: {:?}",
                service_check_id, err
            );
            (
                StatusCode::NOT_FOUND,
                format!("Service check with id={} not found", service_check_id),
            )
        })?;
    let (service_check, service_check_history) = res.into_iter().next().ok_or((
        StatusCode::NOT_FOUND,
        format!("Service check with id={} not found", service_check_id),
    ))?;

    let mut service_check_history = service_check_history;
    service_check_history.sort_by_key(|x| x.timestamp);
    service_check_history.reverse();

    let host = service_check
        .find_related(entities::host::Entity)
        .one(state.db.as_ref())
        .await
        .map_err(|err| {
            error!(
                "Failed to search for service check {}: {:?}",
                service_check.id, err
            );

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Error querying host for service_check={}", service_check_id),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Service check with id={} host not found", service_check_id),
            )
        })?;

    let service = service_check
        .find_related(entities::service::Entity)
        .one(state.db.as_ref())
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "Error querying service for service_check={} error={}",
                    service_check_id, err
                ),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!(
                    "Service check with id={} service not found",
                    service_check_id
                ),
            )
        })?;

    Ok(ServiceCheckTemplate {
        title: format!("Service Check: {}", service_check.id),
        username: Some(user.username()),

        message: None,
        status: "".to_string(),
        service_check,
        host,
        service,
        service_check_history,
    })
}

pub(crate) async fn set_service_check_urgent(
    Path(service_check_id): Path<Uuid>,
    State(state): State<WebState>,
) -> Result<Redirect, impl IntoResponse> {
    set_service_check_status(service_check_id, state, ServiceStatus::Urgent).await
}
pub(crate) async fn set_service_check_disabled(
    Path(service_check_id): Path<Uuid>,
    State(state): State<WebState>,
) -> Result<Redirect, impl IntoResponse> {
    set_service_check_status(service_check_id, state, ServiceStatus::Disabled).await
}

pub(crate) async fn set_service_check_enabled(
    Path(service_check_id): Path<Uuid>,
    State(state): State<WebState>,
) -> Result<Redirect, impl IntoResponse> {
    set_service_check_status(service_check_id, state, ServiceStatus::Pending).await
}

pub(crate) async fn set_service_check_status(
    service_check_id: Uuid,
    state: WebState,
    status: ServiceStatus,
) -> Result<Redirect, impl IntoResponse> {
    let service_check = match entities::service_check::Entity::find_by_id(service_check_id)
        .one(state.db.as_ref())
        .await
    {
        Ok(val) => match val {
            Some(service_check) => service_check,
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    format!("Service check with id={} not found", service_check_id),
                ))
            }
        },
        Err(err) => {
            error!(
                "Failed to search for service check {}: {:?}",
                service_check_id, err
            );
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error".to_string(),
            ));
        }
    };

    let mut service_check = service_check.into_active_model();
    service_check.status.set_if_not_equals(status);
    service_check
        .last_updated
        .set_if_not_equals(chrono::Utc::now());

    let host_id = service_check.host_id.clone().unwrap();

    if service_check.is_changed() {
        service_check.save(state.db.as_ref()).await.map_err(|err| {
            error!(
                "Failed to set service_check_id={} to status={}: {:?}",
                service_check_id, status, err
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error".to_string(),
            )
        })?;
    };
    // TODO: make it so we can redirect to... elsewhere based on a query string?
    Ok(Redirect::to(&format!("/host/{}", host_id.hyphenated())))
}

#[cfg(test)]
mod tests {

    use crate::db::tests::test_setup;

    use super::*;

    #[tokio::test]
    async fn test_view_service_check_without_auth() {
        let state = WebState::test().await;

        let service_check = entities::service_check::Entity::find()
            .one(state.db.as_ref())
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
            .one(state.db.as_ref())
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = service_check_get(
            Path(service_check.id),
            State(state.clone()),
            Some(crate::web::views::tools::test_user_claims()),
        )
        .await
        .expect("Failed to auth!");

        let res = res.to_string();

        dbg!(&res);

        assert!(res.contains("Maremma - Service Check"))
    }

    #[tokio::test]
    async fn test_set_service_check_urgent() {
        let (db, config) = test_setup().await.expect("Failed to set up!");

        let state = WebState::new(db, &config, None);

        let service_check = entities::service_check::Entity::find()
            .one(state.db.as_ref())
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = set_service_check_urgent(Path(service_check.id), State(state.clone())).await;
        assert!(res.is_ok());
        let res = set_service_check_urgent(Path(Uuid::new_v4()), State(state)).await;
        assert!(res.is_err());
    }
    #[tokio::test]
    async fn test_set_service_check_disabled() {
        let state = WebState::test().await;

        let service_check = entities::service_check::Entity::find()
            .one(state.db.as_ref())
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = set_service_check_disabled(Path(service_check.id), State(state.clone())).await;
        assert!(res.is_ok());
        let res = set_service_check_disabled(Path(Uuid::new_v4()), State(state)).await;
        assert!(res.is_err());
    }
    #[tokio::test]
    async fn test_set_service_check_enabled() {
        let (db, config) = test_setup().await.expect("Failed to set up!");

        let state = WebState::new(db, &config, None);

        let service_check = entities::service_check::Entity::find()
            .one(state.db.as_ref())
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = set_service_check_enabled(Path(service_check.id), State(state.clone())).await;
        assert!(res.is_ok());
        let res = set_service_check_enabled(Path(Uuid::new_v4()), State(state)).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_view_missing_service_check_with_auth() {
        use super::*;
        let state = WebState::test().await;

        let mut service_check_id = Uuid::new_v4();
        while entities::service_check::Entity::find_by_id(service_check_id)
            .one(state.db.as_ref())
            .await
            .expect("Failed to search for service_check")
            .is_some()
        {
            service_check_id = Uuid::new_v4();
        }
        let res = super::service_check_get(
            Path(service_check_id),
            State(state.clone()),
            Some(crate::web::views::tools::test_user_claims()),
        )
        .await;

        dbg!(&res);
        assert!(res.is_err());
        assert_eq!(res.into_response().status(), StatusCode::NOT_FOUND)
    }
}
