//! Service-related views

use super::index::SortQueries;
use super::prelude::*;
use crate::constants::{SESSION_CSRF_SCOPE, SESSION_CSRF_TOKEN};
use crate::errors::MaremmaError;
use crate::web::views::csrf::{
    check_csrf_token, consume_csrf_token, issue_csrf_token, service_scope, CsrfTokenForm,
};
use askama_web::WebTemplate;
use axum::Form;
use entities::service_check::FullServiceCheck;
use sea_orm::{ColumnTrait, EntityTrait, ModelTrait, QueryFilter, QueryOrder, TransactionTrait};
use uuid::Uuid;

#[derive(Template, Debug, WebTemplate)]
#[template(path = "service.html")]
pub(crate) struct ServiceTemplate {
    title: String,
    username: Option<String>,
    service: entities::service::Model,
    service_checks: Vec<FullServiceCheck>,
    base_config: String,
    csrf_token: String,
    csrf_scope: String,
}

/// Host view
pub(crate) async fn service(
    Path(service_id): Path<Uuid>,
    State(state): State<WebState>,
    Query(_queries): Query<SortQueries>,
    session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<ServiceTemplate, MaremmaError> {
    let user = check_login(claims)?;

    let db_lock = state.db();

    let service = match entities::service::Entity::find_by_id(service_id)
        .one(db_lock)
        .await
        .map_err(MaremmaError::from)?
    {
        Some(host) => host,
        None => return Err(MaremmaError::ServiceNotFound(service_id)),
    };

    let base_config = serde_json::to_string_pretty(
        &crate::services::Service::try_from_service_model(&service, db_lock).await?,
    )?;
    let service_checks = FullServiceCheck::get_by_service_id(service_id, db_lock).await?;
    let csrf_scope = service_scope(service.id);
    let csrf_token = issue_csrf_token(&session, &csrf_scope).await?;

    Ok(ServiceTemplate {
        title: service.name.clone(),
        service,
        service_checks,
        base_config,
        csrf_token,
        csrf_scope,
        username: Some(user.username()),
    })
}

pub(crate) async fn service_delete(
    Path(service_id): Path<Uuid>,
    State(state): State<WebState>,
    session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    Form(form): Form<CsrfTokenForm>,
) -> Result<Redirect, MaremmaError> {
    check_login(claims)?;
    let scope = service_scope(service_id);
    let allowed_scopes = [scope.as_str()];
    check_csrf_token(
        &form.csrf_token,
        &form.csrf_scope,
        &allowed_scopes,
        &session,
    )
    .await?;

    let db_tx = state.db().begin().await.map_err(MaremmaError::from)?;
    let service = entities::service::Entity::find_by_id(service_id)
        .one(&db_tx)
        .await
        .map_err(MaremmaError::from)?
        .ok_or(MaremmaError::ServiceNotFound(service_id))?;

    service.delete(&db_tx).await.map_err(MaremmaError::from)?;
    db_tx.commit().await.map_err(MaremmaError::from)?;
    consume_csrf_token(
        &form.csrf_token,
        &form.csrf_scope,
        &allowed_scopes,
        &session,
    )
    .await?;

    Ok(Redirect::to(Urls::Services.as_ref()))
}

#[derive(Template, WebTemplate, Debug)]
#[template(path = "services.html")]
pub(crate) struct ServicesTemplate {
    title: String,
    username: Option<String>,
    services: Vec<entities::service::Model>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct ServicesQuery {
    pub(crate) search: Option<String>,
    pub(crate) ord: Option<Order>,
}

pub(crate) async fn services(
    State(state): State<WebState>,
    Query(queries): Query<ServicesQuery>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<ServicesTemplate, MaremmaError> {
    let user = check_login(claims)?;

    let order = queries.ord.unwrap_or(Order::Desc);

    let mut services = entities::service::Entity::find();
    if let Some(search) = queries.search {
        services = services.filter(entities::service::Column::Name.contains(search));
    }

    let services = services
        .order_by(entities::service::Column::Name, order.into())
        .all(state.db())
        .await
        .map_err(MaremmaError::from)?;

    Ok(ServicesTemplate {
        title: "Services".to_string(),
        services,
        username: Some(user.username()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::entities;
    use crate::errors::MaremmaError;
    use crate::web::views::csrf::{issue_csrf_token, service_scope, CsrfTokenForm};
    use crate::web::views::tools::test_user_claims;
    use crate::web::WebState;
    use axum::extract::{Path, Query, State};
    use axum::http::StatusCode;
    use axum::Form;

    #[tokio::test]
    async fn test_view_service_with_auth() {
        let state = WebState::test().await;

        let service = entities::service::Entity::find()
            .one(state.db())
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = super::service(
            Path(service.id),
            State(state.clone()),
            Query(SortQueries::default()),
            state.get_session(),
            Some(crate::web::views::tools::test_user_claims()),
        )
        .await
        .expect("Failed to auth!");

        let res = res.to_string();

        dbg!(&res);

        assert!(res.contains("Maremma"));
        assert!(res.contains("Service config"));
    }
    #[tokio::test]
    async fn test_view_service_without_auth() {
        let state = WebState::test().await;
        let service = entities::service::Entity::find()
            .one(state.db())
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = super::service(
            Path(service.id),
            State(state.clone()),
            Query(SortQueries::default()),
            state.get_session(),
            None,
        )
        .await;

        dbg!(&res);
        assert!(res.is_err());
        assert_eq!(res.into_response().status(), StatusCode::UNAUTHORIZED)
    }
    #[tokio::test]
    async fn test_view_missing_service_with_auth() {
        let state = WebState::test().await;

        let mut service_id = Uuid::new_v4();
        while entities::service::Entity::find_by_id(service_id)
            .one(state.db())
            .await
            .expect("Failed to search for service")
            .is_some()
        {
            service_id = Uuid::new_v4();
        }
        let res = super::service(
            Path(service_id),
            State(state.clone()),
            Query(SortQueries::default()),
            state.get_session(),
            Some(crate::web::views::tools::test_user_claims()),
        )
        .await;

        dbg!(&res);
        assert!(res.is_err());
        assert_eq!(res.into_response().status(), StatusCode::NOT_FOUND)
    }

    #[tokio::test]
    async fn test_view_services_with_auth() {
        use crate::web::test_setup;
        let _ = test_setup().await.expect("failed to setup test");
        let state = WebState::test().await;

        for ord in [
            None,
            Some(crate::web::views::prelude::Order::Asc),
            Some(crate::web::views::prelude::Order::Desc),
        ] {
            let res = super::services(
                State(state.clone()),
                Query(ServicesQuery {
                    search: Some("example".to_string()),
                    ord,
                }),
                Some(test_user_claims()),
            )
            .await;

            dbg!(&res);
            assert!(res.is_ok());

            let response = res.into_response();

            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn test_service_delete_requires_valid_csrf() {
        let state = WebState::test().await;
        let session = state.get_session();
        let service = entities::service::Entity::find()
            .one(state.db())
            .await
            .expect("Failed to query service")
            .expect("No services found");

        let res = service_delete(
            Path(service.id),
            State(state.clone()),
            session.clone(),
            Some(test_user_claims()),
            Form(CsrfTokenForm {
                csrf_token: "wrong".to_string(),
                csrf_scope: service_scope(service.id),
            }),
        )
        .await;
        assert!(res.is_err());
        assert_eq!(
            res.expect_err("Expected csrf error"),
            MaremmaError::CsrfTokenMissing
        );

        let csrf_scope = service_scope(service.id);
        let csrf_token = issue_csrf_token(&session, &csrf_scope)
            .await
            .expect("Failed to issue CSRF token");
        let res = service_delete(
            Path(service.id),
            State(state.clone()),
            session.clone(),
            Some(test_user_claims()),
            Form(CsrfTokenForm {
                csrf_token: format!("{csrf_token}-wrong"),
                csrf_scope,
            }),
        )
        .await;
        assert!(res.is_err());
        assert_eq!(
            res.expect_err("Expected csrf mismatch"),
            MaremmaError::CsrfValidationFailed
        );
    }

    #[tokio::test]
    async fn test_service_delete_success() {
        let state = WebState::test().await;
        let session = state.get_session();
        let service = entities::service::Entity::find()
            .one(state.db())
            .await
            .expect("Failed to query service")
            .expect("No services found");

        let csrf_scope = service_scope(service.id);
        let csrf_token = issue_csrf_token(&session, &csrf_scope)
            .await
            .expect("Failed to issue CSRF token");
        let res = service_delete(
            Path(service.id),
            State(state.clone()),
            session.clone(),
            Some(test_user_claims()),
            Form(CsrfTokenForm {
                csrf_token,
                csrf_scope,
            }),
        )
        .await
        .expect("Failed to delete service");

        assert_eq!(res.into_response().status(), StatusCode::SEE_OTHER);
        assert!(entities::service::Entity::find_by_id(service.id)
            .one(state.db())
            .await
            .expect("Failed to reload service")
            .is_none());
    }
}
