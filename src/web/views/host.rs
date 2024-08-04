use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use super::prelude::*;

use crate::host::HostCheck;
use crate::web::Host;

#[derive(Template)] // this will generate the code...
#[template(path = "host.html")] // using the template in this path, relative
                                // to the `templates` dir in the crate root
pub(crate) struct HostTemplate {
    title: String,
    checks: Vec<entities::service_check::Model>,
    hostname: String,
    check: HostCheck,
    host_groups: Vec<String>,
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

#[axum::debug_handler]
pub(crate) async fn host(
    Path(host_id): Path<Uuid>,
    State(state): State<WebState>,
) -> Result<HostTemplate, impl IntoResponse> {
    let host: Host = match entities::host::Entity::find_by_id(host_id)
        .one(state.db.as_ref())
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
        .unwrap()
    {
        Some(host) => host.into(),
        None => return Err((StatusCode::NOT_FOUND, "Host not found")),
    };

    let checks = entities::service_check::Entity::find()
        .filter(entities::service_check::Column::HostId.eq(host.id))
        .all(state.db.as_ref())
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
        .unwrap();

    let hostname = host.hostname.unwrap_or("unknown hostname".to_string());

    Ok(HostTemplate {
        title: hostname.clone(),
        checks,
        hostname: hostname.clone(),
        check: host.check.clone(),
        host_groups: vec!["test_group1", "test_group2"]
            .into_iter()
            .map(String::from)
            .collect(),
        host_id: host.id,
    })
}
