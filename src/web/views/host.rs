use entities::host_group;
use sea_orm::{ColumnTrait, EntityTrait, ModelTrait, QueryFilter, QueryOrder};
use tracing::error;
use uuid::Uuid;

use super::index::SortQueries;
use super::prelude::*;

use crate::host::HostCheck;
use crate::web::oidc::User;

#[derive(Template, Debug)] // this will generate the code...
#[template(path = "host.html")] // using the template in this path, relative
                                // to the `templates` dir in the crate root
pub(crate) struct HostTemplate {
    title: String,
    username: Option<String>,

    checks: Vec<entities::service_check::FullServiceCheck>,
    hostname: String,
    check: HostCheck,
    host_groups: Vec<host_group::Model>,
    host_id: Uuid,
    page_refresh: u64,
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
        crate::web::views::prelude::OrderFields::LastUpdated => {
            entities::service_check::Column::LastUpdated
        }
        crate::web::views::prelude::OrderFields::Host => entities::service_check::Column::HostId,
        crate::web::views::prelude::OrderFields::Status => entities::service_check::Column::Status,
        crate::web::views::prelude::OrderFields::Check => {
            entities::service_check::Column::LastCheck
        }
    };

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
        .order_by(order_column, queries.ord.unwrap_or_default().into())
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
        username: Some(user.username()),
        page_refresh: 30,
    })
}

#[cfg(test)]
mod tests {

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
}
