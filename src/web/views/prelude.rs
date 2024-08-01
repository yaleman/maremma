// pub(crate) use axum::debug_handler;
pub(crate) use axum::extract::{Path, Query, State};
pub(crate) use chrono::{DateTime, Local};
pub(crate) use serde::Deserialize;

pub(crate) use crate::web::WebState;
pub(crate) use askama_axum::Template;
pub(crate) use std::sync::Arc;

pub(crate) use axum::http::StatusCode;
pub(crate) use axum::response::IntoResponse;

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

#[derive(Eq, PartialEq)]
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
