//! Prometheus metrics magic

use crate::prelude::*;

use opentelemetry::KeyValue;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::Resource;
use prometheus::Registry;

/// Creates the metrics provider and registry for downstream use
pub fn new() -> Result<(SdkMeterProvider, Registry), Error> {
    // create a new prometheus registry
    let registry = prometheus::Registry::new();

    // configure OpenTelemetry to use this registry
    // TODO: work out how to fix this
    // let exporter = opentelemetry_prometheus::exporter()
    //     .with_namespace("maremma")
    //     .with_registry(registry.clone())
    //     .build()
    //     .map_err(|err| Error::Generic(err.to_string()))?;

    let resource = Resource::builder()
        .with_attribute(KeyValue::new("service.name", "maremma"))
        .build();

    // set up a meter to create instruments
    let provider = SdkMeterProvider::builder()
        // .with_reader(exporter)
        .with_resource(resource)
        .build();
    Ok((provider, registry))
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_metrics() {
        let (provider, _registry) = super::new().expect("Failed to create metrics provider");
        provider.shutdown().expect("Failed to shut down");
    }
}
