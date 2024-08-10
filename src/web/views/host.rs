use entities::host_group;
use sea_orm::{ColumnTrait, EntityTrait, ModelTrait, QueryFilter, QueryOrder};
use tracing::error;
use uuid::Uuid;

use super::prelude::*;

use crate::host::HostCheck;
use crate::web::oidc::User;

#[derive(Template)] // this will generate the code...
#[template(path = "host.html")] // using the template in this path, relative
                                // to the `templates` dir in the crate root
pub(crate) struct HostTemplate {
    title: String,
    checks: Vec<entities::service_check::FullServiceCheck>,
    hostname: String,
    check: HostCheck,
    host_groups: Vec<host_group::Model>,
    host_id: Uuid,
    username: Option<String>,
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
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<HostTemplate, impl IntoResponse> {
    let host = match entities::host::Entity::find_by_id(host_id)
        .one(state.db.as_ref())
        .await
    {
        Ok(val) => match val {
            Some(host) => host,
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    format!("Host with id={} not found", host_id),
                ))
            }
        },
        Err(err) => {
            error!("Failed to search for host: {:?}", err);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error".to_string(),
            ));
        }
    };

    use crate::db::entities::service_check::FullServiceCheck;
    let checks = FullServiceCheck::all_query()
        .filter(entities::service_check::Column::HostId.eq(host.id))
        .order_by(
            entities::service_check::Column::LastUpdated,
            sea_orm::Order::Desc, // TODO: make this configurable
        )
        .into_model::<FullServiceCheck>()
        .all(state.db.as_ref())
        .await
        .map_err(|err| {
            error!("Failed to look up service checks for host={host_id} error={err:?}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error".to_string(),
            )
        })?;

    let host_groups = host
        .find_linked(entities::host_group_members::HostToGroups)
        .all(state.db.as_ref())
        .await
        .map_err(|err| {
            error!("Failed to find linked: {:?}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to complete request".to_string(),
            )
        })?;

    Ok(HostTemplate {
        title: host.hostname.to_owned(),
        checks,
        hostname: host.hostname.to_owned(),
        check: host.check,
        host_groups,
        host_id: host.id,
        username: claims.map(|u| User::from(u).username()),
    })
}
