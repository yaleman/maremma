//! Because loads of use statements is messy.

pub use std::collections::HashMap;
pub use std::sync::Arc;
pub use tokio::sync::RwLock;

pub use chrono::{DateTime, Duration, Local, TimeDelta, Utc};
pub use croner::Cron;

pub use async_trait::async_trait;
pub use serde::{Deserialize, Serialize};
pub use serde_json::{json, Value};

pub use tracing::{debug, error, info, instrument, trace, warn};
pub use uuid::Uuid;

pub(crate) use crate::check_loop::CheckResult;
pub(crate) use crate::LOCAL_SERVICE_HOST_NAME;

pub(crate) use crate::config::Configuration;
pub(crate) use crate::db::entities::{self, MaremmaEntity};
pub(crate) use crate::errors::Error;
pub(crate) use crate::host::GenericHost;
pub(crate) use crate::host::Host;
pub(crate) use crate::services::{Service, ServiceStatus, ServiceTrait, ServiceType};

pub(crate) use sea_orm::entity::prelude::*;
pub(crate) use sea_orm::DatabaseConnection;
pub(crate) use sea_orm::IntoActiveModel;

pub(crate) use opentelemetry::metrics::Meter;

pub(crate) use schemars::schema::RootSchema;
pub(crate) use schemars::{schema_for, JsonSchema};
