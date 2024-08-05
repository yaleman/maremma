use entities::service_check::FullServiceCheck;
use sea_orm::{Order, QueryOrder};

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

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum FieldOrder {
    Asc,
    #[default]
    Desc,
}

impl From<FieldOrder> for Order {
    fn from(value: FieldOrder) -> Self {
        match value {
            FieldOrder::Asc => Order::Asc,
            FieldOrder::Desc => Order::Desc,
        }
    }
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub(crate) struct IndexQueries {
    pub ord: Option<FieldOrder>,
    pub field: Option<OrderFields>,
}

#[instrument(level = "info", skip(state))]
pub(crate) async fn index(
    Query(queries): Query<IndexQueries>,
    State(state): State<WebState>,
) -> Result<IndexTemplate, (StatusCode, String)> {
    let sort_order: Order = queries.ord.unwrap_or_default().into();
    let order_field = queries.field.unwrap_or(OrderFields::LastUpdated);
    info!("Sorting home page by: {:?} {:?}", order_field, sort_order);

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
        title: "Maremma".to_string(),
        num_checks: checks.len(),
        checks,
        page_refresh: 90,
    })
}
