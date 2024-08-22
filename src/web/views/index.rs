use entities::service_check::FullServiceCheck;
use sea_orm::{Order as SeaOrmOrder, QueryOrder};

use super::prelude::*;

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub title: String,
    pub num_checks: usize,
    pub checks: Vec<FullServiceCheck>,
    pub page_refresh: u64,
    pub username: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
pub(crate) struct SortQueries {
    pub ord: Option<Order>,
    pub field: Option<OrderFields>,
}

#[instrument(level = "info", skip(state, claims), fields(http.uri="/", ))]
pub(crate) async fn index(
    Query(queries): Query<SortQueries>,
    State(state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<IndexTemplate, (StatusCode, String)> {
    let sort_order: SeaOrmOrder = queries.ord.unwrap_or_default().into();
    let order_field = queries.field.unwrap_or(OrderFields::LastUpdated);
    debug!("Sorting home page by: {:?} {:?}", order_field, sort_order);

    let mut checks = FullServiceCheck::all_query();
    checks = match order_field {
        OrderFields::LastUpdated => {
            checks.order_by(entities::service_check::Column::LastUpdated, sort_order)
        }
        OrderFields::Host => checks.order_by(entities::host::Column::Name, sort_order),
        OrderFields::Status => {
            checks.order_by(entities::service_check::Column::LastUpdated, sort_order)
        }
        OrderFields::Check => checks.order_by(entities::service::Column::Name, sort_order),
    };

    let checks = checks
        .into_model()
        .all(state.db.as_ref())
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    Ok(IndexTemplate {
        title: "".to_string(),
        num_checks: checks.len(),
        checks,
        page_refresh: 90,
        username: claims.map(|c: OidcClaims<EmptyAdditionalClaims>| User::from(c).username()),
    })
}

#[cfg(test)]
mod tests {

    use crate::web::views::tools::test_user_claims;

    use super::*;

    #[tokio::test]
    async fn test_index() {
        let state = WebState::test().await;
        let res = index(
            Query(SortQueries {
                ord: None,
                field: None,
            }),
            State(state),
            None,
        )
        .await;
        assert!(res.is_ok());

        assert!(res.unwrap().to_string().contains("Maremma"));
    }

    #[tokio::test]
    async fn test_index_auth() {
        let state = WebState::test().await;
        let res = index(
            Query(SortQueries {
                ord: None,
                field: None,
            }),
            State(state),
            Some(test_user_claims()),
        )
        .await;
        assert!(res.is_ok());

        assert!(res.unwrap().to_string().contains("Maremma"));
    }
}
