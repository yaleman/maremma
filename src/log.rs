//! Logging and OpenTelemetry tracing setup.

use std::env;
use std::fmt::{Display, Formatter};

use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{filter::LevelFilter, EnvFilter, Registry};

#[cfg(not(test))]
const OTLP_ENDPOINT_VARIABLES: [&str; 2] = [
    "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
    "OTEL_EXPORTER_OTLP_ENDPOINT",
];

#[derive(Debug)]
/// A failure while configuring local or exported telemetry.
pub enum LoggingError {
    /// The configured OTLP exporter could not be built.
    Exporter(opentelemetry_otlp::ExporterBuildError),
    /// Another process-global tracing subscriber is already installed.
    Subscriber(tracing_subscriber::util::TryInitError),
    /// Tokio console integration cannot share this process-global subscriber.
    TokioConsoleUnsupported,
}

impl Display for LoggingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exporter(error) => write!(formatter, "failed to build OTLP exporter: {error}"),
            Self::Subscriber(error) => {
                write!(formatter, "failed to install tracing subscriber: {error}")
            }
            Self::TokioConsoleUnsupported => {
                write!(formatter, "Tokio console integration is not supported")
            }
        }
    }
}

impl std::error::Error for LoggingError {}

/// Keeps the trace provider alive and flushes pending spans during shutdown.
pub struct TelemetryGuard {
    tracer_provider: Option<SdkTracerProvider>,
}

impl TelemetryGuard {
    /// Flushes and shuts down the configured trace provider.
    pub fn shutdown(&self) {
        if let Some(provider) = &self.tracer_provider {
            if let Err(error) = provider.shutdown() {
                eprintln!("Failed to shut down OpenTelemetry tracing: {error}");
            }
        }
    }
}

#[cfg(not(test))]
fn otlp_tracing_enabled() -> bool {
    OTLP_ENDPOINT_VARIABLES
        .iter()
        .any(|variable| env::var_os(variable).is_some_and(|value| !value.is_empty()))
}

#[cfg(test)]
fn otlp_tracing_enabled() -> bool {
    false
}

fn logging_filter(debug: bool, db_debug: bool) -> EnvFilter {
    let default_level = if debug { "debug" } else { "info" };
    let default_directive = if debug {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };
    let mut directives = vec![env::var("RUST_LOG").unwrap_or_else(|_| default_level.to_string())];
    directives.extend([
        "ssh::channel::local::channel=warn".to_string(),
        "h2=warn".to_string(),
        "tower_http::trace::on_request=warn".to_string(),
        "tower_http::trace::on_response=warn".to_string(),
        "tracing::span=warn".to_string(),
    ]);

    if !debug {
        directives.push("ssh=warn".to_string());
    }
    if !db_debug {
        directives.extend([
            "sea_orm::driver::sqlx_sqlite=error".to_string(),
            "sqlx::query=warn".to_string(),
        ]);
    }

    EnvFilter::builder()
        .with_default_directive(default_directive.into())
        .parse_lossy(directives.join(","))
}

/// Sets up stdout logging and optional OTLP trace export.
///
/// OTLP export is enabled when `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` or
/// `OTEL_EXPORTER_OTLP_ENDPOINT` is set. The exporter also honours the standard
/// OTLP protocol, timeout, and header environment variables.
pub fn setup_logging(
    debug: bool,
    db_debug: bool,
    tokio_console: bool,
) -> Result<TelemetryGuard, LoggingError> {
    if tokio_console {
        return Err(LoggingError::TokioConsoleUnsupported);
    }

    let tracer_provider = if otlp_tracing_enabled() {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .build()
            .map_err(LoggingError::Exporter)?;
        let provider = SdkTracerProvider::builder()
            .with_resource(
                Resource::builder()
                    .with_attributes([
                        KeyValue::new("service.name", "maremma"),
                        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                    ])
                    .build(),
            )
            .with_batch_exporter(exporter)
            .build();
        Some(provider)
    } else {
        None
    };

    let telemetry_layer = tracer_provider
        .as_ref()
        .map(|provider| tracing_opentelemetry::layer().with_tracer(provider.tracer("maremma")));
    let subscriber = Registry::default()
        .with(logging_filter(debug, db_debug))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
        .with(telemetry_layer);

    #[cfg(not(test))]
    subscriber.try_init().map_err(LoggingError::Subscriber)?;

    #[cfg(test)]
    if let Err(error) = subscriber.try_init() {
        eprintln!("Error initializing logging: {error}");
    }

    Ok(TelemetryGuard { tracer_provider })
}

#[cfg(test)]
mod tests {
    use super::{logging_filter, otlp_tracing_enabled, setup_logging};

    #[test]
    fn test_setup_logging() {
        assert!(setup_logging(false, true, false).is_ok());
        assert!(setup_logging(true, true, false).is_ok());
        assert!(setup_logging(true, false, false).is_ok());
    }

    #[test]
    fn logging_filter_accepts_default_configuration() {
        let _filter = logging_filter(false, false);
    }

    #[test]
    fn otlp_tracing_is_disabled_without_an_endpoint() {
        assert!(!otlp_tracing_enabled());
    }
}
