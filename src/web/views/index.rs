use std::sync::Arc;

use super::prelude::*;

#[derive(Template)] // this will generate the code...
#[template(path = "index.html")] // using the template in this path, relative
                                 // to the `templates` dir in the crate root
pub struct IndexTemplate {
    // the name of the struct can be anything
    title: String,
    num_checks: usize,
    checks: Vec<Check>,
    page_refresh: u64,
}

#[derive(Deserialize)]
pub(crate) struct IndexQueries {
    pub ord: Option<Order>,
    pub field: Option<OrderFields>,
}

// #[debug_handler]
pub(crate) async fn index(
    Query(queries): Query<IndexQueries>,
    State(state): State<WebState>,
) -> IndexTemplate {
    let check_reader = state.configuration.service_checks.read().await;

    let order_field = queries.field.unwrap_or(OrderFields::LastUpdated);

    let mut checks: Vec<Check> = check_reader
        .values()
        .map(|check| {
            let hostname = state
                .configuration
                .get_host(&check.host_id)
                .map(|host| Arc::new(host.hostname()))
                .unwrap_or(check.host_id.clone());
            let name = state
                .configuration
                .get_service(&check.service_id)
                .map(|service| service.name.clone())
                .unwrap_or_else(|| check.service_id.clone());

            let ordervalue = match order_field {
                OrderFields::LastUpdated => check.last_updated.to_string(),
                OrderFields::Host => hostname.to_lowercase(),
                OrderFields::Status => {
                    format!("{}:{}", check.status, hostname.to_lowercase())
                }
                OrderFields::Check => {
                    format!("{}:{}", name.to_lowercase(), hostname.to_lowercase())
                }
            };

            Check {
                ordervalue,
                host_id: check.host_id.clone(),
                hostname,
                name,
                status: check.status.to_string(),
                last_updated: check.last_updated.into(),
            }
        })
        .collect();
    // do the sorting
    checks.sort_by_key(|check| check.ordervalue.clone());
    // reverse if needed
    if let Order::Desc = queries.ord.unwrap_or(Order::Desc) {
        checks.reverse();
    }

    IndexTemplate {
        title: "Maremma".to_string(),
        num_checks: state.configuration.service_checks.read().await.len(),
        checks,
        page_refresh: 90,
    }
}
