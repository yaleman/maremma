use crate::prelude::*;

use super::prelude::*;

use prometheus::{Encoder, TextEncoder};

pub(crate) async fn metrics(State(state): State<WebState>) -> Result<String, crate::errors::Error> {
    match state.registry {
        Some(registry) => {
            // Ok(Json(format!("{:?}", webmetrics.get_metrics()))),

            let encoder = TextEncoder::new();
            let metric_families = registry.gather();
            let mut result = Vec::new();
            encoder
                .encode(&metric_families, &mut result)
                .map_err(|err| Error::Generic(err.to_string()))?;
            Ok(String::from_utf8(result).map_err(|err| Error::Generic(err.to_string()))?)
        }
        None => Err(crate::errors::Error::NotImplemented),
    }
}
