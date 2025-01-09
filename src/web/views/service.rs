//! Service-related views

use super::index::SortQueries;
use super::prelude::*;
use crate::errors::Error;
use entities::service_check::FullServiceCheck;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use uuid::Uuid;

#[derive(Template, Debug)]
#[template(path = "service.html")]
pub(crate) struct ServiceTemplate {
    title: String,
    username: Option<String>,
    service: entities::service::Model,
    service_checks: Vec<FullServiceCheck>,
}

/// Host view
pub(crate) async fn service(
    Path(service_id): Path<Uuid>,
    State(state): State<WebState>,
    Query(_queries): Query<SortQueries>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<ServiceTemplate, (StatusCode, String)> {
    let user = check_login(claims)?;

    let reader = state.db.read().await;

    let service = match entities::service::Entity::find_by_id(service_id)
        .one(&*reader)
        .await
        .map_err(Error::from)?
    {
        Some(host) => host,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                format!("Service with id={} not found", service_id),
            ))
        }
    };

    let service_checks = FullServiceCheck::get_by_service_id(service_id, &*reader)
        .await
        .map_err(Error::from)?;

    Ok(ServiceTemplate {
        title: service.name.clone(),
        service,
        service_checks,
        username: Some(user.username()),
    })
}

#[derive(Template, Debug)]
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
) -> Result<ServicesTemplate, (StatusCode, String)> {
    let user = check_login(claims)?;

    let order = queries.ord.unwrap_or(Order::Desc);

    let mut services = entities::service::Entity::find();
    if let Some(search) = queries.search {
        services = services.filter(entities::service::Column::Name.contains(search));
    }

    let services = services
        .order_by(entities::service::Column::Name, order.into())
        .all(&*state.db.read().await)
        .await
        .map_err(Error::from)?;

    Ok(ServicesTemplate {
        title: "Services".to_string(),
        services,
        username: Some(user.username()),
    })
}

#[cfg(test)]
mod tests {
    use crate::web::views::tools::test_user_claims;

    #[tokio::test]
    async fn test_view_service_with_auth() {
        use super::*;
        let state = WebState::test().await;

        let service = entities::service::Entity::find()
            .one(&*state.db.read().await)
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = super::service(
            Path(service.id),
            State(state.clone()),
            Query(SortQueries::default()),
            Some(crate::web::views::tools::test_user_claims()),
        )
        .await
        .expect("Failed to auth!");

        let res = res.to_string();

        dbg!(&res);

        assert!(res.contains("Maremma"))
    }
    #[tokio::test]
    async fn test_view_service_without_auth() {
        use super::*;
        let state = WebState::test().await;
        let service = entities::service::Entity::find()
            .one(&*state.db.read().await)
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = super::service(
            Path(service.id),
            State(state.clone()),
            Query(SortQueries::default()),
            None,
        )
        .await;

        dbg!(&res);
        assert!(res.is_err());
        assert_eq!(res.into_response().status(), StatusCode::UNAUTHORIZED)
    }
    #[tokio::test]
    async fn test_view_missing_service_with_auth() {
        use super::*;
        let state = WebState::test().await;

        let mut service_id = Uuid::new_v4();
        while entities::service::Entity::find_by_id(service_id)
            .one(&*state.db.read().await)
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
            Some(crate::web::views::tools::test_user_claims()),
        )
        .await;

        dbg!(&res);
        assert!(res.is_err());
        assert_eq!(res.into_response().status(), StatusCode::NOT_FOUND)
    }

    #[tokio::test]
    async fn test_view_services_with_auth() {
        use super::*;
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
}
