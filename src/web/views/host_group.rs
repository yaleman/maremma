//! Host Group Related views
//!

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Redirect;
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

use crate::db::entities::{host, host_group, host_group_members};
use crate::web::oidc::User;
use crate::web::{Error, WebState};

#[derive(Template)]
#[template(path = "host_groups.html")]
pub(crate) struct HostGroupsTemplate {
    title: String,
    username: Option<String>,
    host_groups: Vec<HostGroupData>,
}

pub(crate) struct HostGroupData {
    id: Uuid,
    name: String,
    hosts: usize,
}

pub(crate) async fn host_groups(
    State(state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<HostGroupsTemplate, (StatusCode, String)> {
    if claims.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized".to_string()));
    }
    let res = host_group::Entity::find()
        .order_by_asc(host_group::Column::Name)
        .find_with_linked(host_group_members::GroupToHosts)
        .all(state.db.as_ref())
        .await
        .map_err(|e| {
            log::error!("Failed to fetch host groups: {}", e);
            Error::from(e)
        })?;

    let host_groups = res
        .into_iter()
        .map(|(group, hosts)| HostGroupData {
            id: group.id,
            name: group.name,
            hosts: hosts.len(),
        })
        .collect();

    Ok(HostGroupsTemplate {
        title: "Host Groups".to_string(),
        username: None,
        host_groups,
    })
}

#[derive(Template)]
#[template(path = "host_group.html")]
pub(crate) struct HostGroupTemplate {
    title: String,
    username: Option<String>,
    host_group: host_group::Model,
    members: Vec<host::Model>,
    message: Option<String>,
}

#[derive(Deserialize, Default)]
pub(crate) struct HostGroupQueries {
    pub ord: Option<super::prelude::Order>,
    pub message: Option<String>,
}

pub(crate) async fn host_group(
    Path(id): Path<Uuid>,
    Query(query): Query<HostGroupQueries>,
    State(state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<HostGroupTemplate, (StatusCode, String)> {
    if claims.is_none() {
        // TODO: check that the user is an admin
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized".to_string()));
    }

    let host_group = host_group::Entity::find()
        .filter(host_group::Column::Id.eq(id))
        .find_with_linked(host_group_members::GroupToHosts)
        .all(state.db.as_ref())
        .await
        .map_err(|e| {
            log::error!("Failed to fetch host groups: {}", e);
            Error::from(e)
        })?;

    let (host_group, mut members) = match host_group.into_iter().next() {
        Some(val) => val,
        None => return Err((StatusCode::NOT_FOUND, "Host Group not found".to_string())),
    };

    match query.ord.unwrap_or(super::prelude::Order::Asc) {
        super::prelude::Order::Asc => members.sort_by(|a, b| a.hostname.cmp(&b.hostname)),
        super::prelude::Order::Desc => members.sort_by(|a, b| b.hostname.cmp(&a.hostname)),
    };

    Ok(HostGroupTemplate {
        title: format!("Host Group: {}", host_group.name),
        username: None,
        host_group,
        members,
        message: query.message,
    })
}

pub(crate) async fn host_group_member_delete(
    Path((group_id, host_id)): Path<(Uuid, Uuid)>,
    State(state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<Redirect, (StatusCode, String)> {
    let user: User = match claims {
        None => {
            // TODO: check that the user is an admin
            return Err((StatusCode::UNAUTHORIZED, "Unauthorized".to_string()));
        }
        Some(val) => val.into(),
    };

    let host = host::Entity::find_by_id(host_id)
        .one(state.db.as_ref())
        .await
        .map_err(|e| {
            log::error!("Failed to fetch host: {}", e);
            Error::from(e)
        })?;
    let host = match host {
        Some(val) => val,
        None => {
            return Err((StatusCode::NOT_FOUND, "Host not found".to_string()));
        }
    };

    let group = host_group::Entity::find_by_id(group_id)
        .one(state.db.as_ref())
        .await
        .map_err(|e| {
            log::error!("Failed to fetch host: {}", e);
            Error::from(e)
        })?;
    let group = match group {
        Some(val) => val,
        None => {
            return Err((StatusCode::NOT_FOUND, "Group not found".to_string()));
        }
    };

    let host_group_membership = host_group_members::Entity::delete_many()
        .filter(
            host_group_members::Column::GroupId
                .eq(group_id)
                .and(host_group_members::Column::HostId.eq(host_id)),
        )
        .exec(state.db.as_ref())
        .await
        .map_err(|e| {
            log::error!("Failed to delete host group membership: {}", e);
            Error::from(e)
        })?;
    info!(
        "user={} Deleted {} host_group_membership row host_id={} group_id={}",
        user.username(),
        host_group_membership.rows_affected,
        host_id.hyphenated(),
        group_id.hyphenated()
    );

    Ok(Redirect::to(&format!(
        "/host_group/{}?message=Removed {} from '{}'",
        group_id, host.hostname, group.name
    )))
}

pub(crate) async fn host_group_delete(
    Path(group_id): Path<Uuid>,
    State(state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<Redirect, (StatusCode, String)> {
    let _user: User = match claims {
        None => {
            // TODO: check that the user is an admin
            return Err((StatusCode::UNAUTHORIZED, "Unauthorized".to_string()));
        }
        Some(val) => val.into(),
    };

    host_group::Entity::delete_by_id(group_id)
        .exec(state.db.as_ref())
        .await
        .map_err(|e| {
            log::error!("Failed to delete host group: {}", e);
            Error::from(e)
        })?;

    Ok(Redirect::to("/host_groups"))
}

#[cfg(test)]
mod tests {
    use askama_axum::IntoResponse;
    use axum::extract::{Path, Query, State};
    use uuid::Uuid;

    use crate::db::tests::test_setup;
    use crate::web::views::host_group::HostGroupQueries;
    use crate::web::WebState;

    #[tokio::test]
    async fn test_unauthed_endpoints() {
        let (_db, _config) = test_setup().await.expect("Failed to setup test harness");
        let state = WebState::test().await;

        let res = super::host_groups(State(state.clone()), None).await;
        assert!(res.is_err());
        assert_eq!(
            res.into_response().status(),
            axum::http::StatusCode::UNAUTHORIZED
        );

        let res = super::host_group(
            Path(Uuid::new_v4()),
            Query(HostGroupQueries::default()),
            State(state.clone()),
            None,
        )
        .await;
        assert!(res.is_err());
        assert_eq!(
            res.into_response().status(),
            axum::http::StatusCode::UNAUTHORIZED
        );

        let res = super::host_group_member_delete(
            Path((Uuid::new_v4(), Uuid::new_v4())),
            State(state.clone()),
            None,
        )
        .await;
        assert!(res.is_err());
        assert_eq!(
            res.into_response().status(),
            axum::http::StatusCode::UNAUTHORIZED
        )
    }
}
