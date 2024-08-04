use entities::service_check::FullServiceCheck;
use sea_orm::QueryOrder;

use super::prelude::*;

#[derive(Template)] // this will generate the code...
#[template(path = "index.html")]
pub struct IndexTemplate {
    // the name of the struct can be anything
    pub title: String,
    pub num_checks: usize,
    pub checks: Vec<FullServiceCheck>,
    pub page_refresh: u64,
}

#[allow(dead_code)]
#[derive(Deserialize)]
pub(crate) struct IndexQueries {
    pub ord: Option<Order>,
    pub field: Option<OrderFields>,
}

// #[debug_handler]
pub(crate) async fn index(
    Query(queries): Query<IndexQueries>,
    State(state): State<WebState>,
) -> Result<IndexTemplate, (StatusCode, String)> {
    let order_by_field = entities::service_check::Column::LastUpdated;

    let checks: Vec<FullServiceCheck> = FullServiceCheck::all_query()
        .order_by(order_by_field, queries.ord.unwrap_or(Order::Desc).into())
        .into_model()
        .all(state.db.as_ref())
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    Ok(IndexTemplate {
        title: "Maremma".to_string(),
        num_checks: checks.len(),
        checks,
        page_refresh: 90,
    })
}
