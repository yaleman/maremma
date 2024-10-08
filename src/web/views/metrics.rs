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

#[cfg(test)]
mod tests {
    #[tokio::test]

    async fn test_metrics_view() {
        use super::*;
        let state = WebState::test().await.with_registry();

        let res = super::metrics(State(state.clone())).await;

        dbg!(&res);
        assert!(res.is_ok());
        assert_eq!(res.into_response().status(), StatusCode::OK)
    }
    #[tokio::test]
    async fn test_metrics_without_registry() {
        use super::*;
        let state = WebState::test().await;

        let res = super::metrics(State(state.clone())).await;

        dbg!(&res);
        assert!(res.is_err());
    }
}
