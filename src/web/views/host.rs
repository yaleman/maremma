use sea_orm::{ColumnTrait, DbErr, EntityTrait, QueryFilter};
use tracing::error;
use uuid::Uuid;

use super::prelude::*;

use crate::host::HostCheck;

#[derive(Template)] // this will generate the code...
#[template(path = "host.html")] // using the template in this path, relative
                                // to the `templates` dir in the crate root
pub(crate) struct HostTemplate {
    title: String,
    checks: Vec<entities::service_check::Model>,
    hostname: String,
    check: HostCheck,
    host_groups: Vec<Uuid>,
    host_id: Uuid,
}

#[derive(Default, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Order {
    Asc,
    #[default]
    Desc,
}

#[derive(Default, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OrderFields {
    #[default]
    LastUpdated,
    Host,
    Status,
    Check,
}

pub(crate) async fn host(
    Path(host_id): Path<Uuid>,
    State(state): State<WebState>,
) -> Result<HostTemplate, impl IntoResponse> {
    let host = match entities::host::Entity::find_by_id(host_id)
        .one(state.db.as_ref())
        .await
    {
        Ok(val) => val,
        Err(DbErr::RecordNotFound(_)) => None,
        Err(err) => {
            error!("Failed to search for host: {:?}", err);
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "Database error"));
        }
    };

    let host = match host {
        Some(host) => host,
        None => return Err((StatusCode::NOT_FOUND, "Host not found")),
    };

    let checks = entities::service_check::Entity::find()
        .filter(entities::service_check::Column::HostId.eq(host.id))
        .all(state.db.as_ref())
        .await
        .map_err(|err| {
            error!("Failed to look up service checks for host={host_id} error={err:?}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        })?;

    // TODO: change this from the UUIDs to the host group models
    let host_groups = entities::host_group_members::Entity::find()
        .filter(entities::host_group_members::Column::HostId.eq(host.id))
        .all(state.db.as_ref())
        .await
        .map_err(|err| {
            error!("Failed to look up host_groups for host={host_id} error={err:?}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        })?
        .into_iter()
        .map(|hgm| hgm.group_id)
        .collect();

    Ok(HostTemplate {
        title: host.hostname.clone(),
        checks,
        hostname: host.hostname.clone(),
        check: host.check.clone(),
        host_groups,
        host_id: host.id,
    })
}
