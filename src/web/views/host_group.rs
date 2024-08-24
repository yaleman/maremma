//! Host Group Related views
//!

use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Redirect;
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use sea_orm::{EntityTrait, ModelTrait, QueryOrder};
use uuid::Uuid;

use crate::db::entities;
use crate::db::entities::host_group::{Column, Entity, Model};
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
}

pub(crate) async fn host_group(
    Path(id): Path<Uuid>,
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

    let members = host_group
        .find_linked(entities::host_group_members::GroupToHosts)
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
    })
}

pub(crate) async fn host_group_member_delete(
    Path((_group_id, _host_id)): Path<(Uuid, Uuid)>,
    State(_state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<Redirect, (StatusCode, String)> {
    if claims.is_none() {
        // TODO: check that the user is an admin
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized".to_string()));
    }

    todo!()
}

#[cfg(test)]
mod tests {
    use askama_axum::IntoResponse;
    use axum::extract::{Path, State};
    use uuid::Uuid;

    use crate::db::tests::test_setup;
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

        let res = super::host_group(Path(Uuid::new_v4()), State(state.clone()), None).await;
        assert!(res.is_err());
        assert_eq!(
            res.into_response().status(),
            axum::http::StatusCode::UNAUTHORIZED
        )
    }
}
