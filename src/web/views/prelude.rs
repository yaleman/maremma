pub(crate) use crate::db::entities;
pub(crate) use crate::services::ServiceStatus;
pub(crate) use crate::web::WebState;
pub(crate) use askama_axum::Template;
pub(crate) use axum::extract::{Path, Query, State};
pub(crate) use axum::response::Redirect;
pub(crate) use chrono::{DateTime, Local};
pub(crate) use serde::Deserialize;
pub(crate) use std::sync::Arc;

pub(crate) use axum::http::StatusCode;
pub(crate) use axum::response::IntoResponse;
pub(crate) use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel};
pub(crate) use uuid::Uuid;

pub(crate) use tracing::*;

#[derive(Default, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Order {
    Asc,
    #[default]
    Desc,
}

impl From<Order> for sea_orm::Order {
    fn from(value: Order) -> Self {
        match value {
            Order::Asc => sea_orm::Order::Asc,
            Order::Desc => sea_orm::Order::Desc,
        }
    }
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

#[allow(dead_code)]
#[derive(Eq, PartialEq)]
/// used in askama templates for displaying checks
pub(crate) struct Check {
    /// Used internally for sorting the checks
    pub ordervalue: String,
    pub host_id: Arc<String>,
    pub hostname: Arc<String>,
    pub name: String,
    pub status: String,
    pub last_updated: DateTime<Local>,
}

impl Ord for Check {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.ordervalue.cmp(&other.ordervalue)
    }
}

impl PartialOrd for Check {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_from() {
        assert_eq!(sea_orm::Order::Asc, Order::Asc.into());
        assert_eq!(sea_orm::Order::Desc, Order::Desc.into());
    }

    #[test]
    fn test_check_ord() {
        let check1 = Check {
            ordervalue: "1".to_string(),
            host_id: Arc::new("1".to_string()),
            hostname: Arc::new("host1".to_string()),
            name: "check1".to_string(),
            status: "OK".to_string(),
            last_updated: Local::now(),
        };
        let check2 = Check {
            ordervalue: "2".to_string(),
            host_id: Arc::new("2".to_string()),
            hostname: Arc::new("host2".to_string()),
            name: "check2".to_string(),
            status: "OK".to_string(),
            last_updated: Local::now(),
        };
        assert_eq!(check1.cmp(&check2), std::cmp::Ordering::Less);
        assert_eq!(check1.partial_cmp(&check2), Some(std::cmp::Ordering::Less));
    }
}
