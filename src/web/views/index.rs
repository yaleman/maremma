use askama_web::WebTemplate;
use entities::service_check::FullServiceCheck;
use sea_orm::{ColumnTrait, Order as SeaOrmOrder, QueryFilter, QueryOrder};

use crate::errors::Error;

use super::prelude::*;

#[derive(Template, WebTemplate)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub title: String,
    pub num_checks: usize,
    pub checks: Vec<FullServiceCheck>,
    pub page_refresh: u64,
    pub username: Option<String>,
    pub search: String,
    pub ord: Order,
    pub field: OrderFields,
}

#[derive(Deserialize, Debug, Default)]
pub(crate) struct SortQueries {
    pub ord: Option<Order>,
    pub field: Option<OrderFields>,
    pub search: Option<String>,
}

#[instrument(level = "info", skip(state, claims), fields(http.uri=Urls::Index.as_ref(), ))]
pub(crate) async fn index(
    Query(queries): Query<SortQueries>,
    State(state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<IndexTemplate, (StatusCode, String)> {
    let sort_order: SeaOrmOrder = queries.ord.unwrap_or_default().into();
    let order_field = queries.field.unwrap_or(OrderFields::Status);
    debug!("Sorting home page by: {:?} {:?}", order_field, sort_order);

    let mut checks = FullServiceCheck::all_query();
    if let Some(search) = &queries.search {
        checks = checks.filter(
            entities::service::Column::Name
                .contains(search)
                .or(entities::host::Column::Name.contains(search))
                .or(entities::service_check::Column::Status.contains(search)),
        );
    }
    checks = match order_field {
        OrderFields::LastUpdated => checks.order_by(
            entities::service_check::Column::LastUpdated,
            sort_order.clone(),
        ),
        OrderFields::Service => {
            checks.order_by(entities::service::Column::Name, sort_order.clone())
        }
        OrderFields::Host => checks.order_by(entities::host::Column::Name, sort_order.clone()),
        OrderFields::Status => checks.order_by(
            entities::service_check::Column::LastUpdated,
            sort_order.clone(),
        ),
        OrderFields::Check => checks.order_by(entities::service::Column::Name, sort_order.clone()),
        OrderFields::NextCheck => checks.order_by(
            entities::service_check::Column::NextCheck,
            sort_order.clone(),
        ),
    };
    debug!("Getting reader...");
    let db_lock = state.get_db_lock().await;
    debug!("got reader");
    let mut checks = checks
        .into_model()
        .all(&*db_lock)
        .await
        .map_err(Error::from)?;
    drop(db_lock);
    debug!("query done");

    if order_field == OrderFields::Status {
        checks.sort_by(|a: &FullServiceCheck, b: &FullServiceCheck| a.status.cmp(&b.status));
        if sort_order == SeaOrmOrder::Desc {
            checks.reverse();
        }
    }

    Ok(IndexTemplate {
        title: "".to_string(),
        num_checks: checks.len(),
        checks,
        page_refresh: 90,
        username: claims.map(|c| User::from(c).username()),
        search: queries.search.unwrap_or_default(),
        ord: queries.ord.unwrap_or_default(),
        field: order_field,
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
                search: None,
            }),
            State(state),
            None,
        )
        .await;
        assert!(res.is_ok());

        assert!(res
            .expect("Failed to get response")
            .to_string()
            .contains("Maremma"));
    }

    #[tokio::test]
    async fn test_index_auth() {
        let state = WebState::test().await;
        let res = index(
            Query(SortQueries {
                ord: None,
                field: None,
                search: None,
            }),
            State(state),
            Some(test_user_claims()),
        )
        .await;
        assert!(res.is_ok());

        assert!(res
            .expect("failed to get response")
            .to_string()
            .contains("Maremma"));
    }

    #[tokio::test]
    async fn test_index_search() {
        let state = WebState::test().await;
        let res = index(
            Query(SortQueries {
                ord: None,
                field: None,
                search: Some("example.com".to_string()),
            }),
            State(state),
            None,
        )
        .await;
        assert!(res.is_ok());

        let page_content = res.expect("Failed to get response body").to_string();

        assert!(page_content.contains("example.com"));
        assert!(!page_content.contains("local_lslah"));
    }
}
