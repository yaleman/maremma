pub(crate) use crate::db::entities;
pub(crate) use crate::services::ServiceStatus;
pub(crate) use crate::web::oidc::User;
pub(crate) use crate::web::urls::Urls;
pub(crate) use crate::web::WebState;

pub(crate) use askama_axum::Template;
pub(crate) use axum::extract::{Path, Query, State};
pub(crate) use axum::response::Redirect;
pub(crate) use chrono::{DateTime, Local};
use sea_orm::EnumIter;
pub(crate) use serde::Deserialize;
use serde::Serialize;
use std::fmt::Display;
pub(crate) use std::sync::Arc;

pub(crate) use axum::http::StatusCode;
pub(crate) use axum::response::IntoResponse;
pub(crate) use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel};
pub(crate) use uuid::Uuid;

pub(crate) use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
pub(crate) use tracing::*;

#[derive(Default, Deserialize, Debug, Copy, Clone, EnumIter)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Order {
    Asc,
    #[default]
    Desc,
}

impl Order {
    #[cfg(test)]
    pub(crate) fn iter_all_and_none() -> Vec<Option<Self>> {
        use sea_orm::Iterable;

        let mut v = Self::iter().map(Some).collect::<Vec<Option<Self>>>();
        v.push(None);
        v
    }
}

impl std::fmt::Display for Order {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Order::Asc => write!(f, "asc"),
            Order::Desc => write!(f, "desc"),
        }
    }
}

impl From<Order> for sea_orm::Order {
    fn from(value: Order) -> Self {
        match value {
            Order::Asc => sea_orm::Order::Asc,
            Order::Desc => sea_orm::Order::Desc,
        }
    }
}

#[derive(Default, Deserialize, Serialize, Debug, Copy, Clone, EnumIter, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OrderFields {
    #[default]
    LastUpdated,
    Host,
    Service,
    Status,
    Check,
    NextCheck,
}

impl Display for OrderFields {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderFields::LastUpdated => write!(f, "last_updated"),
            OrderFields::Host => write!(f, "host"),
            OrderFields::Service => write!(f, "service"),
            OrderFields::Status => write!(f, "status"),
            OrderFields::Check => write!(f, "check"),
            OrderFields::NextCheck => write!(f, "next_check"),
        }
    }
}

impl OrderFields {
    #[cfg(test)]
    pub(crate) fn iter_all_and_none() -> Vec<Option<Self>> {
        use sea_orm::Iterable;

        let mut v = Self::iter().map(Some).collect::<Vec<Option<Self>>>();
        v.push(None);
        v
    }
}

#[derive(Eq, PartialEq)]
/// used in Askama templates for displaying checks
pub struct Check {
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

pub(crate) fn check_login(
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<User, (StatusCode, String)> {
    match claims {
        Some(user) => Ok(User::from(user)),
        None => Err((
            StatusCode::UNAUTHORIZED,
            "You must be logged in to view this page".to_string(),
        )),
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
