//! Because loads of use statements is messy.

pub(crate) use crate::check_loop::CheckResult;
pub(crate) use crate::config::{Configuration, SendableConfig};
pub(crate) use crate::db::entities::{self, MaremmaEntity};
#[cfg(test)]
pub(crate) use crate::db::tests::test_setup;
pub(crate) use crate::errors::MaremmaError;
pub(crate) use crate::host::GenericHost;
pub(crate) use crate::host::Host;
pub(crate) use crate::services::{Service, ServiceStatus, ServiceTrait, ServiceType};
pub(crate) use crate::web::urls::Urls;
pub(crate) use crate::LOCAL_SERVICE_HOST_NAME;
pub use async_trait::async_trait;
pub use chrono::{DateTime, Duration, Local, TimeDelta, Utc};
pub use croner::Cron;
pub(crate) use opentelemetry::metrics::Meter;
pub(crate) use schemars::{schema_for, JsonSchema};
pub use sea_orm::prelude::{
    ActiveModelBehavior, ActiveModelTrait, ColumnTrait, ColumnTypeTrait, ConnectionTrait,
    DeriveActiveEnum, DeriveDisplay, DeriveEntityModel, DerivePrimaryKey, DeriveRelation,
    EntityTrait, EnumIter, Expr, Json, Linked, ModelTrait, PrimaryKeyToColumn, PrimaryKeyTrait,
    QueryFilter, Related, RelationDef, RelationTrait, Select, StringLen,
};
pub(crate) use sea_orm::DatabaseConnection;
pub use sea_orm::IntoActiveModel;
pub use serde::{Deserialize, Serialize};
pub use serde_json::{json, Map, Value};
pub use std::collections::HashMap;
pub use std::sync::Arc;
pub use tokio::sync::RwLock;
pub use tracing::{debug, error, info, instrument, trace, warn};
pub use uuid::Uuid;
