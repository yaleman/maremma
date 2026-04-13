use super::index::SortQueries;
use super::prelude::*;
use crate::prelude::*;

use crate::constants::{SESSION_CSRF_SCOPE, SESSION_CSRF_TOKEN};
use crate::db::entities::service_check::FullServiceCheck;
use crate::web::views::csrf::{
    check_csrf_token, consume_csrf_token, host_scope, issue_csrf_token, CsrfTokenForm,
};
use axum::Form;
use entities::host_group;
use sea_orm::{ColumnTrait, EntityTrait, ModelTrait, QueryFilter, QueryOrder, TransactionTrait};

#[derive(Template, Debug, WebTemplate)]
#[template(path = "host.html")]
pub(crate) struct HostTemplate {
    title: String,
    username: Option<String>,
    host: entities::host::Model,
    checks: Vec<entities::service_check::FullServiceCheck>,
    host_groups: Vec<host_group::Model>,
    page_refresh: u64,
    csrf_token: String,
    csrf_scope: String,
}

/// Host view
pub(crate) async fn host(
    Path(host_id): Path<Uuid>,
    State(state): State<WebState>,
    Query(queries): Query<SortQueries>,
    session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<HostTemplate, MaremmaError> {
    let user = check_login(claims)?;

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

    let db_lock = state.db();

    let (host, host_groups) = match entities::host::Entity::find_by_id(host_id)
        .find_with_linked(entities::host_group_members::HostToGroups)
        .all(db_lock)
        .await
        .map_err(MaremmaError::from)?
        .into_iter()
        .next()
    {
        Some(host) => host,
        None => {
            return Err(MaremmaError::HostNotFound(host_id));
        }
    };

    let checks = FullServiceCheck::all_query()
        .filter(entities::service_check::Column::HostId.eq(host.id))
        .order_by(order_column, queries.ord.unwrap_or_default().into())
        .into_model::<FullServiceCheck>()
        .all(db_lock)
        .await
        .map_err(|err| {
            error!("Failed to look up service checks for host={host_id} error={err:?}");
            MaremmaError::from(err)
        })?;

    let csrf_scope = host_scope(host.id);
    let csrf_token = issue_csrf_token(&session, &csrf_scope).await?;

    Ok(HostTemplate {
        title: host.hostname.to_owned(),
        checks,
        host,
        host_groups,
        username: Some(user.username()),
        page_refresh: 30,
        csrf_token,
        csrf_scope,
    })
}

#[derive(Template, WebTemplate)]
#[template(path = "hosts.html")]
pub(crate) struct HostsTemplate {
    title: String,
    username: Option<String>,
    hosts: Vec<HostListItem>,
    search_string: String,
}

#[derive(Debug)]
pub(crate) struct HostListItem {
    host: entities::host::Model,
    status: ServiceStatus,
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
) -> Result<HostsTemplate, MaremmaError> {
    let user = check_login(claims)?;
    let order_field = queries.queries.field.unwrap_or(OrderFields::Status);
    let order = queries.queries.ord.unwrap_or(super::prelude::Order::Desc);

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

    let hosts = hosts
        .all(state.db.as_ref())
        .await
        .map_err(MaremmaError::from)?;

    let host_statuses = aggregate_host_statuses(
        &hosts.iter().map(|host| host.id).collect::<Vec<Uuid>>(),
        state.db.as_ref(),
    )
    .await?;

    let mut hosts = hosts
        .into_iter()
        .map(|host| HostListItem {
            status: host_statuses
                .get(&host.id)
                .copied()
                .unwrap_or(ServiceStatus::Unknown),
            host,
        })
        .collect::<Vec<HostListItem>>();

    sort_host_list_items(&mut hosts, order_field, order);

    Ok(HostsTemplate {
        title: "Hosts".to_string(),
        username: Some(user.username()),
        hosts,
        search_string: queries.search.unwrap_or_default(),
    })
}

async fn aggregate_host_statuses(
    host_ids: &[Uuid],
    db: &DatabaseConnection,
) -> Result<HashMap<Uuid, ServiceStatus>, MaremmaError> {
    let mut statuses = HashMap::new();

    for service_check in entities::service_check::Entity::find()
        .filter(entities::service_check::Column::HostId.is_in(host_ids.iter().copied()))
        .all(db)
        .await
        .map_err(MaremmaError::from)?
    {
        statuses
            .entry(service_check.host_id)
            .and_modify(|status| {
                if service_check.status > *status {
                    *status = service_check.status;
                }
            })
            .or_insert(service_check.status);
    }

    Ok(statuses)
}

fn sort_host_list_items(
    hosts: &mut [HostListItem],
    field: OrderFields,
    ord: crate::web::views::prelude::Order,
) {
    hosts.sort_by(|left, right| {
        match field {
            OrderFields::Status => {
                let status_ordering = match ord {
                    crate::web::views::prelude::Order::Asc => left.status.cmp(&right.status),
                    crate::web::views::prelude::Order::Desc => right.status.cmp(&left.status),
                };

                status_ordering
                    .then_with(|| left.host.hostname.cmp(&right.host.hostname))
                    .then_with(|| left.host.name.cmp(&right.host.name))
            }
            OrderFields::Host
            | OrderFields::Service
            | OrderFields::LastUpdated
            | OrderFields::NextCheck
            | OrderFields::Check => left
                .host
                .hostname
                .cmp(&right.host.hostname)
                .then_with(|| left.host.name.cmp(&right.host.name))
                .then_with(|| left.status.cmp(&right.status)),
        }
    });

    if field != OrderFields::Status
        && matches!(ord, crate::web::views::prelude::Order::Desc)
    {
        hosts.reverse();
    }
}

pub(crate) async fn delete_host(
    State(state): State<WebState>,
    Path(host_id): Path<Uuid>,
    session: Session,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    Form(csrf_form): Form<CsrfTokenForm>,
) -> Result<Redirect, MaremmaError> {
    check_login(claims)?;
    let scope = host_scope(host_id);
    let allowed_scopes = [scope.as_str()];
    check_csrf_token(
        &csrf_form.csrf_token,
        &csrf_form.csrf_scope,
        &allowed_scopes,
        &session,
    )
    .await?;

    let db_writer = state.db().begin().await.map_err(MaremmaError::from)?;
    let host = match entities::host::Entity::find_by_id(host_id)
        .one(&db_writer)
        .await
        .map_err(MaremmaError::from)?
    {
        Some(host) => host,
        None => {
            return Err(MaremmaError::HostNotFound(host_id));
        }
    };

    host.delete(&db_writer).await.map_err(MaremmaError::from)?;
    db_writer.commit().await.map_err(MaremmaError::from)?;
    consume_csrf_token(
        &csrf_form.csrf_token,
        &csrf_form.csrf_scope,
        &allowed_scopes,
        &session,
    )
    .await?;

    Ok(Redirect::to(Urls::Hosts.as_ref()))
}

#[cfg(test)]
mod tests {
    use crate::web::views::tools::test_user_claims;

    #[tokio::test]
    async fn test_view_host_with_auth() {
        use super::*;
        let _ = test_setup().await.expect("Failed to set up test");
        let state = WebState::test().await;

        let host = entities::host::Entity::find()
            .one(state.db())
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
            .one(state.db())
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
            .one(state.db())
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

                    let rendered = res.expect("Failed to render hosts page").to_string();

                    assert!(rendered.contains("Status"));
                    assert!(rendered.contains("Unknown"));

                    let response = rendered.into_response();

                    assert_eq!(response.status(), StatusCode::OK);
                }
            }
        }
    }

    #[test]
    fn test_sort_host_list_items_status_tie_breaks_by_hostname() {
        use super::*;

        let mut hosts = vec![
            HostListItem {
                host: entities::host::Model {
                    id: Uuid::new_v4(),
                    name: "Zulu".to_string(),
                    hostname: "zulu.example.test".to_string(),
                    check: crate::host::HostCheck::None,
                    config: serde_json::json!({}),
                },
                status: ServiceStatus::Critical,
            },
            HostListItem {
                host: entities::host::Model {
                    id: Uuid::new_v4(),
                    name: "Alpha".to_string(),
                    hostname: "alpha.example.test".to_string(),
                    check: crate::host::HostCheck::None,
                    config: serde_json::json!({}),
                },
                status: ServiceStatus::Critical,
            },
        ];

        sort_host_list_items(
            &mut hosts,
            OrderFields::Status,
            crate::web::views::prelude::Order::Desc,
        );

        assert_eq!(hosts[0].host.hostname, "alpha.example.test");
        assert_eq!(hosts[1].host.hostname, "zulu.example.test");
    }

    #[tokio::test]
    async fn test_view_hosts_defaults_to_status_desc() {
        use super::*;

        let _ = test_setup().await.expect("Failed to set up test");
        let state = WebState::test().await;

        let example_host = entities::host::Entity::find()
            .filter(entities::host::Column::Name.eq("example.com"))
            .one(state.db())
            .await
            .expect("Failed to search for example host")
            .expect("Missing example host");
        let local_host = entities::host::Entity::find()
            .filter(entities::host::Column::Name.eq(crate::LOCAL_SERVICE_HOST_NAME))
            .one(state.db())
            .await
            .expect("Failed to search for local host")
            .expect("Missing local host");

        for service_check in entities::service_check::Entity::find()
            .filter(entities::service_check::Column::HostId.eq(example_host.id))
            .all(state.db())
            .await
            .expect("Failed to load example host checks")
        {
            service_check
                .set_status(ServiceStatus::Critical, state.db())
                .await
                .expect("Failed to set example host status");
        }

        for service_check in entities::service_check::Entity::find()
            .filter(entities::service_check::Column::HostId.eq(local_host.id))
            .all(state.db())
            .await
            .expect("Failed to load local host checks")
        {
            service_check
                .set_status(ServiceStatus::Ok, state.db())
                .await
                .expect("Failed to set local host status");
        }

        let res = hosts(
            State(state.clone()),
            Query(HostsQuery {
                search: None,
                queries: SortQueries::default(),
            }),
            state.get_session(),
            Some(test_user_claims()),
        )
        .await
        .expect("Failed to render hosts page");

        let ordered_hosts = res
            .hosts
            .iter()
            .map(|host| (host.host.name.clone(), host.status))
            .collect::<Vec<(String, ServiceStatus)>>();

        assert_eq!(
            ordered_hosts,
            vec![
                ("example.com".to_string(), ServiceStatus::Critical),
                (crate::LOCAL_SERVICE_HOST_NAME.to_string(), ServiceStatus::Ok),
            ]
        );
    }

    #[tokio::test]
    async fn test_view_delete_host_with_auth() {
        use super::*;
        let _ = test_setup().await.expect("Failed to set up test");
        let state = WebState::test().await;

        let host = entities::host::Entity::find()
            .one(state.db())
            .await
            .expect("Failed to search for host")
            .expect("No host found");

        let session = state.get_session();
        let csrf_scope = host_scope(host.id);
        let csrf_token = issue_csrf_token(&session, &csrf_scope)
            .await
            .expect("Failed to save CSRF token");

        let res = super::delete_host(
            State(state.clone()),
            Path(host.id),
            session.clone(),
            Some(test_user_claims()),
            Form(CsrfTokenForm {
                csrf_token,
                csrf_scope: csrf_scope.clone(),
            }),
        )
        .await;

        assert!(res.is_ok());
        let response = res.into_response();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        // test deleting a non-existent host

        let mut nonexistent_host_id = Uuid::new_v4();
        while entities::host::Entity::find_by_id(nonexistent_host_id)
            .one(state.db())
            .await
            .expect("Failed to search for host")
            .is_some()
        {
            nonexistent_host_id = Uuid::new_v4();
        }

        let session = state.get_session();
        let csrf_scope = host_scope(nonexistent_host_id);
        let csrf_token = issue_csrf_token(&session, &csrf_scope)
            .await
            .expect("Failed to save CSRF token");

        let res = super::delete_host(
            State(state.clone()),
            Path(nonexistent_host_id),
            session,
            Some(test_user_claims()),
            Form(CsrfTokenForm {
                csrf_token,
                csrf_scope,
            }),
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
            Form(CsrfTokenForm {
                csrf_token: "test".to_string(),
                csrf_scope: host_scope(Uuid::new_v4()),
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
        let host_id = Uuid::new_v4();
        let res = super::delete_host(
            State(state.clone()),
            Path(host_id),
            state.get_session(),
            Some(test_user_claims()),
            Form(CsrfTokenForm {
                csrf_token: "test".to_string(),
                csrf_scope: host_scope(host_id),
            }),
        )
        .await;

        assert!(res.is_err());

        if let Err(err) = &res {
            assert_eq!(err, &MaremmaError::CsrfTokenMissing);
        } else {
            panic!("Should have gotten an error!")
        };

        let response = res.into_response();
        dbg!(&response);
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let session = state.get_session();
        let host_id = Uuid::new_v4();
        let csrf_scope = host_scope(host_id);
        issue_csrf_token(&session, &csrf_scope)
            .await
            .expect("Failed to save CSRF token");

        // test with a CSRF token in the store, but user specifies the wrong one
        let res = super::delete_host(
            State(state.clone()),
            Path(host_id),
            session,
            Some(test_user_claims()),
            Form(CsrfTokenForm {
                csrf_token: "test".to_string(),
                csrf_scope,
            }),
        )
        .await;

        assert!(res.is_err());

        if let Err(err) = &res {
            assert_eq!(err, &MaremmaError::CsrfValidationFailed);
        } else {
            panic!("Should have gotten an error!")
        }

        let response = res.into_response();
        dbg!(&response);
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
