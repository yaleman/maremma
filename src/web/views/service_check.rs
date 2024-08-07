use super::prelude::*;

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

    let host_id = service_check.host_id.clone();

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
    Ok(Redirect::to(&format!(
        "/host/{}",
        host_id.as_ref().hyphenated()
    )))
}

#[cfg(test)]
mod tests {

    use crate::db::tests::test_setup;

    use super::*;

    #[tokio::test]
    async fn test_set_service_check_urgent() {
        let (db, config) = test_setup().await.expect("Failed to set up!");

        let state = WebState::new(db, &config);

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
        let (db, config) = test_setup().await.expect("Failed to set up!");

        let state = WebState::new(db, &config);

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

        let state = WebState::new(db, &config);

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
}
