use opentelemetry::{global, KeyValue};
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::{runtime, Resource};

use opentelemetry_stdout::MetricsExporter;

fn build_exporter() -> MetricsExporter {
    opentelemetry_stdout::MetricsExporterBuilder::default()
        // uncomment the below lines to pretty print output.
        //  .with_encoder(|writer, data|/
        //    Ok(serde_json::to_writer_pretty(writer, &data).unwrap()))
        .build()
}

pub fn init_meter_provider() -> opentelemetry_sdk::metrics::SdkMeterProvider {
    let reader = PeriodicReader::builder(build_exporter(), runtime::Tokio).build();
    let provider = SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(Resource::new([KeyValue::new("service.name", "maremma")]))
        .build();
    global::set_meter_provider(provider.clone());
    provider
}
