use super::index::SortQueries;
use super::prelude::*;
use crate::errors::Error;
use entities::host_group;
use sea_orm::{ColumnTrait, EntityTrait, ModelTrait, QueryFilter, QueryOrder};
use tracing::error;
use uuid::Uuid;

use crate::web::oidc::User;

#[derive(Template, Debug)] // this will generate the code...
#[template(path = "host.html")] // using the template in this path, relative
                                // to the `templates` dir in the crate root
pub(crate) struct HostTemplate {
    title: String,
    username: Option<String>,
    host: entities::host::Model,
    checks: Vec<entities::service_check::FullServiceCheck>,
    host_groups: Vec<host_group::Model>,
    page_refresh: u64,
}

#[derive(Default, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Order {
    Asc,
    #[default]
    Desc,
}

/// Host view
pub(crate) async fn host(
    Path(host_id): Path<Uuid>,
    State(state): State<WebState>,
    Query(queries): Query<SortQueries>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<HostTemplate, (StatusCode, String)> {
    let user = claims.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            "You must be logged in to view this page".to_string(),
        )
    })?;

    let user: User = user.into();
    let order_field = queries
        .field
        .unwrap_or(crate::web::views::prelude::OrderFields::LastUpdated);
    let order_column = match order_field {
        OrderFields::LastUpdated => entities::service_check::Column::LastUpdated,
        OrderFields::Host => entities::service_check::Column::HostId,
        OrderFields::Status => entities::service_check::Column::Status,
        OrderFields::Check => entities::service_check::Column::LastCheck,
        OrderFields::NextCheck => entities::service_check::Column::NextCheck,
    };

    let (host, host_groups) = match entities::host::Entity::find_by_id(host_id)
        .find_with_linked(entities::host_group_members::HostToGroups)
        .all(state.db.as_ref())
        .await
        .map_err(Error::from)?
        .into_iter()
        .next()
    {
        Some(host) => host,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                format!("Host with id={} not found", host_id),
            ))
        }
    };

    use crate::db::entities::service_check::FullServiceCheck;
    let checks = FullServiceCheck::all_query()
        .filter(entities::service_check::Column::HostId.eq(host.id))
        .order_by(order_column, queries.ord.unwrap_or_default().into())
        .into_model::<FullServiceCheck>()
        .all(state.db.as_ref())
        .await
        .map_err(|err| {
            error!("Failed to look up service checks for host={host_id} error={err:?}");
            Error::from(err)
        })?;

    Ok(HostTemplate {
        title: host.hostname.to_owned(),
        checks,
        host,
        host_groups,
        username: Some(user.username()),
        page_refresh: 30,
    })
}

#[derive(Template)]
#[template(path = "hosts.html")]
pub(crate) struct HostsTemplate {
    title: String,
    username: Option<String>,
    hosts: Vec<entities::host::Model>,
    search_string: String,
}

#[derive(Deserialize, Debug, Default)]
pub(crate) struct HostsQuery {
    search: Option<String>,
    #[serde(flatten)]
    queries: SortQueries,
}

pub(crate) async fn hosts(
    State(state): State<WebState>,
    Query(queries): Query<HostsQuery>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<HostsTemplate, (StatusCode, String)> {
    let user = claims.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            "You must be logged in to view this page".to_string(),
        )
    })?;
    let user: User = user.into();

    let mut hosts = entities::host::Entity::find();
    if let Some(search_string) = queries.search.clone() {
        if !search_string.trim().is_empty() {
            let search_string = format!("%{}%", search_string.trim().replace(" ", "%"));
            hosts = hosts.filter(
                entities::host::Column::Hostname
                    .like(search_string.clone())
                    .or(entities::host::Column::Name.like(search_string)),
            );
        }
    }

    let ord = queries.queries.ord.unwrap_or(super::prelude::Order::Asc);
    let order_column = match queries.queries.field.unwrap_or_default() {
        OrderFields::Host => entities::host::Column::Hostname,
        OrderFields::LastUpdated => entities::host::Column::Hostname,
        OrderFields::NextCheck => entities::host::Column::Hostname,
        OrderFields::Status => entities::host::Column::Check,
        OrderFields::Check => entities::host::Column::Check,
    };
    let hosts = hosts
        .order_by(order_column, ord.into())
        .all(state.db.as_ref())
        .await
        .map_err(Error::from)?;

    Ok(HostsTemplate {
        title: "Hosts".to_string(),
        username: Some(user.username()),
        hosts,
        search_string: queries.search.unwrap_or_default(),
    })
}

pub(crate) async fn delete_host(
    State(state): State<WebState>,
    Path(host_id): Path<Uuid>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<Redirect, (StatusCode, String)> {
    let _user = claims.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            "You must be logged in to view this page".to_string(),
        )
    })?;

    let host = match entities::host::Entity::find_by_id(host_id)
        .one(state.db.as_ref())
        .await
        .map_err(Error::from)?
    {
        Some(host) => host,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                format!("Host with id={} not found", host_id),
            ))
        }
    };

    host.delete(state.db.as_ref()).await.map_err(Error::from)?;
    Ok(Redirect::to("/hosts"))
}

#[cfg(test)]
mod tests {
    use crate::web::views::tools::test_user_claims;

    #[tokio::test]
    async fn test_view_host_with_auth() {
        use super::*;
        let state = WebState::test().await;

        let host = entities::host::Entity::find()
            .one(state.db.as_ref())
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = super::host(
            Path(host.id),
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
    async fn test_view_host_without_auth() {
        use super::*;
        let state = WebState::test().await;
        let host = entities::host::Entity::find()
            .one(state.db.as_ref())
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = super::host(
            Path(host.id),
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
    async fn test_view_missing_host_with_auth() {
        use super::*;
        let state = WebState::test().await;

        let mut host_id = Uuid::new_v4();
        while entities::host::Entity::find_by_id(host_id)
            .one(state.db.as_ref())
            .await
            .expect("Failed to search for host")
            .is_some()
        {
            host_id = Uuid::new_v4();
        }
        let res = super::host(
            Path(host_id),
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
    async fn test_view_hosts_with_auth() {
        use super::*;
        let state = WebState::test().await;

        for field in [
            Some(OrderFields::Host),
            Some(OrderFields::LastUpdated),
            Some(OrderFields::NextCheck),
            Some(OrderFields::Status),
            Some(OrderFields::Check),
            None,
        ]
        .into_iter()
        {
            for ord in [
                None,
                Some(crate::web::views::prelude::Order::Asc),
                Some(crate::web::views::prelude::Order::Desc),
            ] {
                let res = super::hosts(
                    State(state.clone()),
                    Query(HostsQuery {
                        search: Some("example".to_string()),
                        queries: SortQueries { field, ord },
                    }),
                    Some(test_user_claims()),
                )
                .await;

                assert!(res.is_ok());

                let response = res.into_response();

                assert_eq!(response.status(), StatusCode::OK);
            }
        }
    }

    #[tokio::test]
    async fn test_view_delete_host_with_auth() {
        use super::*;
        let state = WebState::test().await;

        let host = entities::host::Entity::find()
            .one(state.db.as_ref())
            .await
            .expect("Failed to search for host")
            .expect("No host found");

        let res = super::delete_host(
            State(state.clone()),
            Path(host.id),
            Some(test_user_claims()),
        )
        .await;

        assert!(res.is_ok());

        let response = res.into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }
    #[tokio::test]
    async fn test_view_delete_host_without_auth() {
        use super::*;
        let state = WebState::test().await;

        let res = super::delete_host(State(state.clone()), Path(Uuid::new_v4()), None).await;

        assert!(res.is_err());

        let response = res.into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
