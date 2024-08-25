//! Host Group Related views
//!

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Redirect;
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use sea_orm::{ColumnTrait, EntityTrait, ModelTrait, QueryFilter, QueryOrder};
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

use crate::db::entities;
use crate::db::entities::host_group::{Column, Entity, Model};
use crate::web::oidc::User;
use crate::web::{Error, WebState};

#[derive(Template)]
#[template(path = "host_groups.html")]
pub(crate) struct HostGroupsTemplate {
    title: String,
    username: Option<String>,
    host_groups: Vec<Model>,
}

pub(crate) async fn host_groups(
    State(state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<HostGroupsTemplate, (StatusCode, String)> {
    if claims.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized".to_string()));
    }
    let host_groups = Entity::find()
        .order_by_asc(Column::Name)
        .all(state.db.as_ref())
        .await
        .map_err(|e| {
            log::error!("Failed to fetch host groups: {}", e);
            Error::from(e)
        })?;

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
    host_group: entities::host_group::Model,
    members: Vec<entities::host::Model>,
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

    let host_group = entities::host_group::Entity::find_by_id(id)
        .one(state.db.as_ref())
        .await
        .map_err(|e| {
            log::error!("Failed to fetch host groups: {}", e);
            Error::from(e)
        })?;

    let host_group = match host_group {
        Some(val) => val,
        None => return Err((StatusCode::NOT_FOUND, "Host Group not found".to_string())),
    };

    let query_sort = query.ord.unwrap_or(super::prelude::Order::Asc).into();
    let members = host_group
        .find_linked(entities::host_group_members::GroupToHosts)
        .order_by(entities::host::Column::Hostname, query_sort)
        .all(state.db.as_ref())
        .await
        .map_err(|e| {
            log::error!("Failed to fetch host groups: {}", e);
            Error::from(e)
        })?;

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

    let host = entities::host::Entity::find_by_id(host_id)
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

    let group = entities::host_group::Entity::find_by_id(group_id)
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

    let host_group_membership = entities::host_group_members::Entity::delete_many()
        .filter(
            entities::host_group_members::Column::GroupId
                .eq(group_id)
                .and(entities::host_group_members::Column::HostId.eq(host_id)),
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

    // TODO: unpick the group -> service -> host -> check chain

    Ok(Redirect::to(&format!(
        "/host_group/{}?message=Removed {} from '{}'",
        group_id, host.hostname, group.name
    )))
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
