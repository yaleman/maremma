use super::index::SortQueries;
use super::prelude::*;

use crate::constants::{CSRF_TOKEN_MISMATCH, CSRF_TOKEN_NOT_FOUND, SESSION_CSRF_TOKEN};
use crate::db::entities::service_check::FullServiceCheck;
use crate::errors::Error;
use axum::Form;
use entities::host_group;
use sea_orm::{ColumnTrait, EntityTrait, ModelTrait, QueryFilter, QueryOrder};
use uuid::Uuid;

#[derive(Template, Debug)]
#[template(path = "host.html")]
pub(crate) struct HostTemplate {
    title: String,
    username: Option<String>,
    host: entities::host::Model,
    checks: Vec<entities::service_check::FullServiceCheck>,
    host_groups: Vec<host_group::Model>,
    page_refresh: u64,
    csrf_token: String,
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
    session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let user = check_login(claims)?;

    let csrf_token = state.new_csrf_token();
    session
        .insert(SESSION_CSRF_TOKEN, &csrf_token)
        .await
        .map_err(Error::from)?;

    let order_field = queries
        .field
        .unwrap_or(crate::web::views::prelude::OrderFields::LastUpdated);
    let order_column = match order_field {
        OrderFields::LastUpdated => entities::service_check::Column::LastUpdated,
        OrderFields::Host => entities::service_check::Column::HostId,
        OrderFields::Service => entities::service_check::Column::ServiceId,
        OrderFields::Status => entities::service_check::Column::Status,
        OrderFields::Check => entities::service_check::Column::LastCheck,
        OrderFields::NextCheck => entities::service_check::Column::NextCheck,
    };

    let db_reader = state.db.read().await;

    let (host, host_groups) = match entities::host::Entity::find_by_id(host_id)
        .find_with_linked(entities::host_group_members::HostToGroups)
        .all(&*db_reader)
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

    let checks = FullServiceCheck::all_query()
        .filter(entities::service_check::Column::HostId.eq(host.id))
        .order_by(order_column, queries.ord.unwrap_or_default().into())
        .into_model::<FullServiceCheck>()
        .all(&*db_reader)
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
        csrf_token,
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
    pub(crate) search: Option<String>,
    #[serde(flatten)]
    pub(crate) queries: SortQueries,
}

pub(crate) async fn hosts(
    State(state): State<WebState>,
    Query(queries): Query<HostsQuery>,
    _session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<HostsTemplate, (StatusCode, String)> {
    let user = check_login(claims)?;

    let mut hosts = entities::host::Entity::find();
    if let Some(search_string) = &queries.search {
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
        OrderFields::Service => entities::host::Column::Hostname,
        OrderFields::LastUpdated => entities::host::Column::Hostname,
        OrderFields::NextCheck => entities::host::Column::Hostname,
        OrderFields::Status => entities::host::Column::Check,
        OrderFields::Check => entities::host::Column::Check,
    };
    let hosts = hosts
        .order_by(order_column, ord.into())
        .all(&*state.db.read().await)
        .await
        .map_err(Error::from)?;

    Ok(HostsTemplate {
        title: "Hosts".to_string(),
        username: Some(user.username()),
        hosts,
        search_string: queries.search.unwrap_or_default(),
    })
}

#[derive(Deserialize, Debug)]
pub struct CsrfForm {
    #[allow(dead_code)]
    pub csrf_token: String,
}

pub(crate) async fn delete_host(
    State(state): State<WebState>,
    Path(host_id): Path<Uuid>,
    session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    Form(csrf_form): Form<CsrfForm>,
) -> Result<Redirect, (StatusCode, String)> {
    let _user = claims.ok_or_else(|| {
        debug!("User not logged in");
        (
            StatusCode::UNAUTHORIZED,
            "You must be logged in to view this page".to_string(),
        )
    })?;

    let session_csrf_token: String = match session
        .remove(SESSION_CSRF_TOKEN)
        .await
        .map_err(Error::from)?
    {
        Some(val) => val,
        None => {
            return Err((StatusCode::FORBIDDEN, CSRF_TOKEN_NOT_FOUND.to_string()));
        }
    };

    if csrf_form.csrf_token != session_csrf_token {
        return Err((StatusCode::FORBIDDEN, CSRF_TOKEN_MISMATCH.to_string()));
    }

    let db_writer = state.db.write().await;
    let host = match entities::host::Entity::find_by_id(host_id)
        .one(&*db_writer)
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

    host.delete(&*db_writer).await.map_err(Error::from)?;
    Ok(Redirect::to(Urls::Hosts.as_ref()))
}

#[cfg(test)]
mod tests {

    use crate::web::test_setup;
    use crate::web::views::tools::test_user_claims;

    #[tokio::test]
    async fn test_view_host_with_auth() {
        use super::*;
        let _ = test_setup().await.expect("Failed to set up test");
        let state = WebState::test().await;

        let host = entities::host::Entity::find()
            .one(&*state.db.read().await)
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        for ord in [
            None,
            Some(crate::web::views::prelude::Order::Asc),
            Some(crate::web::views::prelude::Order::Desc),
        ] {
            for field in [
                None,
                Some(OrderFields::Host),
                Some(OrderFields::Service),
                Some(OrderFields::LastUpdated),
                Some(OrderFields::NextCheck),
                Some(OrderFields::Status),
                Some(OrderFields::Check),
            ] {
                let res = super::host(
                    Path(host.id),
                    State(state.clone()),
                    Query(SortQueries {
                        ord,
                        field,
                        search: None,
                    }),
                    state.get_session(),
                    Some(crate::web::views::tools::test_user_claims()),
                )
                .await;

                assert!(res.is_ok());
            }
        }
    }
    #[tokio::test]
    async fn test_view_host_without_auth() {
        use super::*;
        let _ = test_setup().await.expect("Failed to set up test");
        let state = WebState::test().await;
        let host = entities::host::Entity::find()
            .one(&*state.db.read().await)
            .await
            .expect("Failed to get service check")
            .expect("No service checks found");

        let res = super::host(
            Path(host.id),
            State(state.clone()),
            Query(SortQueries::default()),
            state.get_session(),
            None,
        )
        .await;

        // dbg!(&res);
        assert!(res.is_err());
        assert_eq!(res.into_response().status(), StatusCode::UNAUTHORIZED)
    }
    #[tokio::test]
    async fn test_view_missing_host_with_auth() {
        use super::*;
        let _ = test_setup().await.expect("Failed to set up test");
        let state = WebState::test().await;

        let mut host_id = Uuid::new_v4();
        while entities::host::Entity::find_by_id(host_id)
            .one(&*state.db.read().await)
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
            state.get_session(),
            Some(crate::web::views::tools::test_user_claims()),
        )
        .await;
        assert!(res.is_err());
        assert_eq!(res.into_response().status(), StatusCode::NOT_FOUND)
    }

    #[tokio::test]
    async fn test_view_hosts_with_auth() {
        use super::*;
        let _ = test_setup().await.expect("Failed to set up test");
        let state = WebState::test().await;

        for search in [None, Some("example".to_string())] {
            for field in OrderFields::iter_all_and_none().into_iter() {
                for ord in crate::web::views::prelude::Order::iter_all_and_none().into_iter() {
                    let session = state.get_session();

                    let res = super::hosts(
                        State(state.clone()),
                        Query(HostsQuery {
                            search: search.clone(),
                            queries: SortQueries {
                                field,
                                ord,
                                search: None,
                            },
                        }),
                        session,
                        Some(test_user_claims()),
                    )
                    .await;

                    assert!(res.is_ok());

                    let response = res.into_response();

                    assert_eq!(response.status(), StatusCode::OK);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_view_delete_host_with_auth() {
        use super::*;
        let _ = test_setup().await.expect("Failed to set up test");
        let state = WebState::test().await;

        let host = entities::host::Entity::find()
            .one(&*state.db.read().await)
            .await
            .expect("Failed to search for host")
            .expect("No host found");

        let csrf_token = state.new_csrf_token();
        let session = state.get_session();
        session
            .insert(SESSION_CSRF_TOKEN, &csrf_token)
            .await
            .expect("Failed to save CSRF token");

        let res = super::delete_host(
            State(state.clone()),
            Path(host.id),
            session.clone(),
            Some(test_user_claims()),
            Form(CsrfForm { csrf_token }),
        )
        .await;

        assert!(res.is_ok());
        let response = res.into_response();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        // test deleting a non-existent host

        let session = state.get_session();
        let csrf_token = state.new_csrf_token();
        session
            .insert(SESSION_CSRF_TOKEN, &csrf_token)
            .await
            .expect("Failed to save CSRF token");

        let mut nonexistent_host_id = Uuid::new_v4();
        while entities::host::Entity::find_by_id(nonexistent_host_id)
            .one(&*state.db.read().await)
            .await
            .expect("Failed to search for host")
            .is_some()
        {
            nonexistent_host_id = Uuid::new_v4();
        }

        let res = super::delete_host(
            State(state.clone()),
            Path(nonexistent_host_id),
            session,
            Some(test_user_claims()),
            Form(CsrfForm { csrf_token }),
        )
        .await;

        assert!(res.is_err());

        let response = res.into_response();
        dbg!(&response);

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_view_delete_host_without_auth() {
        use super::*;
        let _ = test_setup().await.expect("Failed to set up test");
        let state = WebState::test().await;

        let res = super::delete_host(
            State(state.clone()),
            Path(Uuid::new_v4()),
            state.get_session(),
            None,
            Form(CsrfForm {
                csrf_token: "test".to_string(),
            }),
        )
        .await;

        assert!(res.is_err());

        let response = res.into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    #[tokio::test]
    async fn test_view_delete_host_with_invalid_csrf() {
        use super::*;
        let _ = test_setup().await.expect("Failed to set up test");
        let state = WebState::test().await;

        // test with no CSRF token in the store
        let res = super::delete_host(
            State(state.clone()),
            Path(Uuid::new_v4()),
            state.get_session(),
            Some(test_user_claims()),
            Form(CsrfForm {
                csrf_token: "test".to_string(),
            }),
        )
        .await;

        assert!(res.is_err());

        match res.clone() {
            Err(err) => {
                assert_eq!(err.0, StatusCode::FORBIDDEN);
                assert_eq!(err.1, CSRF_TOKEN_NOT_FOUND.to_string());
            }
            Ok(_) => panic!("Should have gotten an error!"),
        }

        let response = res.into_response();
        dbg!(&response);
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let session = state.get_session();
        let csrf_token = state.new_csrf_token();
        session
            .insert(SESSION_CSRF_TOKEN, &csrf_token)
            .await
            .expect("Failed to save CSRF token");

        // test with a CSRF token in the store, but user specifies the wrong one
        let res = super::delete_host(
            State(state.clone()),
            Path(Uuid::new_v4()),
            session,
            Some(test_user_claims()),
            Form(CsrfForm {
                csrf_token: "test".to_string(),
            }),
        )
        .await;

        assert!(res.is_err());

        match res.clone() {
            Err(err) => {
                assert_eq!(err.0, StatusCode::FORBIDDEN);
                assert_eq!(err.1, CSRF_TOKEN_MISMATCH.to_string());
            }
            Ok(_) => panic!("Should have gotten an error!"),
        }

        let response = res.into_response();
        dbg!(&response);
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
