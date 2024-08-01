use super::prelude::*;

use crate::host::HostCheck;

#[derive(Template)] // this will generate the code...
#[template(path = "host.html")] // using the template in this path, relative
                                // to the `templates` dir in the crate root
pub(crate) struct HostTemplate {
    title: String,
    checks: Vec<Check>,
    hostname: String,
    check: HostCheck,
    host_groups: Vec<String>,
    host_id: Arc<String>,
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

// #[debug_handler]
pub(crate) async fn host(
    // Query(queries): Query<IndexQueries>,
    Path(host_id): Path<String>,
    State(state): State<WebState>,
) -> Result<HostTemplate, impl IntoResponse> {
    let check_reader = state.configuration.service_checks.read().await;

    let host = match state
        .configuration
        .hosts
        .iter()
        .find(|f| *f.host_id() == host_id)
    {
        Some(host) => host,
        None => return Err((StatusCode::NOT_FOUND, "Host not found")),
    };

    let host_id = Arc::new(host_id);

    let mut checks: Vec<Check> = check_reader
        .values()
        .filter_map(|check| {
            let check_hostname = state
                .configuration
                .get_host(&check.host_id)
                .map(|host| Arc::new(host.hostname()))
                .unwrap_or(check.host_id.clone());

            if *check_hostname != host.hostname() {
                return None;
            }

            let check_name = state
                .configuration
                .get_service(&check.service_id)
                .map(|service| service.name.clone())
                .unwrap_or_else(|| check.service_id.clone());

            Some(Check {
                ordervalue: check_name.to_lowercase(),
                host_id: host_id.clone(),
                hostname: check_hostname,
                name: check_name,
                status: check.status.to_string(),
                last_updated: check.last_updated.into(),
            })
        })
        .collect();
    // do the sorting
    checks.sort_by_key(|check| check.ordervalue.clone());
    // reverse if needed
    // if let Order::Desc = queries.ord.unwrap_or(Order::Desc) {
    //     checks.reverse();
    // }

    Ok(HostTemplate {
        title: "Home".into(),
        checks,
        hostname: host.hostname(),
        check: host.check.clone(),
        host_groups: vec!["test_group1", "test_group2"]
            .into_iter()
            .map(String::from)
            .collect(),
        host_id: host.host_id(),
    })
}
