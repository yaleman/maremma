use entities::service_check::FullServiceCheck;
use sea_orm::{Order as SeaOrmOrder, QueryOrder};

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
#[derive(Deserialize, Debug)]
pub(crate) struct IndexQueries {
    pub ord: Option<Order>,
    pub field: Option<OrderFields>,
}

#[instrument(level = "info", skip(state), fields(http.uri="/", ))]
pub(crate) async fn index(
    Query(queries): Query<IndexQueries>,
    State(state): State<WebState>,
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
    })
}

#[cfg(test)]
mod tests {
    use crate::db::tests::test_setup;

    use super::*;

    #[tokio::test]
    async fn test_index() {
        let (db, _config) = test_setup().await.expect("Failed to set up!");

        let state = WebState::new(db);
        let res = index(
            Query(IndexQueries {
                ord: None,
                field: None,
            }),
            State(state),
        )
        .await;
        assert!(res.is_ok());

        assert!(res.unwrap().to_string().contains("Maremma"));
    }
}
