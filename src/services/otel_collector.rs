//! OpenTelemetry collector service checks through the Kubernetes API proxy.

use std::collections::HashMap;
use std::io;

use http::Request;
use kube::Client;
use prometheus_parse::{Scrape, Value as PrometheusValue};
use schemars::JsonSchema;

use super::prelude::*;
use crate::prelude::*;

const DEFAULT_HEALTH_PORT: u16 = 13133;
const DEFAULT_METRICS_PORT: u16 = 8888;
const DEFAULT_QUEUE_WARNING_PERCENT: f64 = 50.0;
const DEFAULT_QUEUE_CRITICAL_PERCENT: f64 = 80.0;

fn default_health_port() -> u16 {
    DEFAULT_HEALTH_PORT
}

fn default_metrics_port() -> u16 {
    DEFAULT_METRICS_PORT
}

fn default_queue_warning_percent() -> f64 {
    DEFAULT_QUEUE_WARNING_PERCENT
}

fn default_queue_critical_percent() -> f64 {
    DEFAULT_QUEUE_CRITICAL_PERCENT
}

#[derive(Debug, Deserialize, JsonSchema, Serialize, Clone)]
/// A check of an OpenTelemetry collector exposed through a Kubernetes Service.
pub struct OtelCollectorService {
    /// Name of the check.
    pub name: String,
    /// Namespace containing the collector Service.
    pub namespace: String,
    /// Collector Service name.
    pub service_name: String,
    /// Collector health extension port.
    #[serde(default = "default_health_port")]
    pub health_port: u16,
    /// Collector internal Prometheus metrics port.
    #[serde(default = "default_metrics_port")]
    pub metrics_port: u16,
    /// Queue utilization percentage that produces a warning.
    #[serde(default = "default_queue_warning_percent")]
    pub queue_warning_percent: f64,
    /// Queue utilization percentage that produces a critical result.
    #[serde(default = "default_queue_critical_percent")]
    pub queue_critical_percent: f64,
    #[serde(with = "crate::serde::cron")]
    #[schemars(with = "String")]
    /// Schedule for the service.
    pub cron_schedule: Cron,
    /// Add random jitter in 0..n seconds to the check.
    pub jitter: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct HealthResponse {
    status: String,
}

#[derive(Debug, PartialEq)]
struct CollectorMetrics {
    max_queue_percent: f64,
    accepted: f64,
    exported: f64,
    refused: f64,
    enqueue_failed: f64,
    send_failed: f64,
}

impl ConfigOverlay for OtelCollectorService {
    fn overlay_host_config(&self, value: &Map<String, Json>) -> Result<Box<Self>, MaremmaError> {
        Ok(Box::new(Self {
            name: self.extract_string(value, "name", &self.name),
            namespace: self.extract_string(value, "namespace", &self.namespace),
            service_name: self.extract_string(value, "service_name", &self.service_name),
            health_port: self.extract_value(value, "health_port", &self.health_port)?,
            metrics_port: self.extract_value(value, "metrics_port", &self.metrics_port)?,
            queue_warning_percent: self.extract_value(
                value,
                "queue_warning_percent",
                &self.queue_warning_percent,
            )?,
            queue_critical_percent: self.extract_value(
                value,
                "queue_critical_percent",
                &self.queue_critical_percent,
            )?,
            cron_schedule: self.extract_cron(value, "cron_schedule", &self.cron_schedule)?,
            jitter: self.extract_value(value, "jitter", &self.jitter)?,
        }))
    }
}

fn valid_dns_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn proxy_path(namespace: &str, service_name: &str, port: u16, path: &str) -> String {
    format!("/api/v1/namespaces/{namespace}/services/http:{service_name}:{port}/proxy{path}")
}

async fn get_proxy_text(
    client: &Client,
    namespace: &str,
    service_name: &str,
    port: u16,
    path: &str,
) -> Result<String, MaremmaError> {
    let request = Request::get(proxy_path(namespace, service_name, port, path))
        .body(Vec::new())
        .map_err(|error| {
            MaremmaError::Generic(format!("Failed to build proxy request: {error}"))
        })?;
    client
        .request_text(request)
        .await
        .map_err(|error| MaremmaError::Generic(format!("Kubernetes service proxy failed: {error}")))
}

fn metric_value(value: &PrometheusValue) -> Option<f64> {
    match value {
        PrometheusValue::Counter(value)
        | PrometheusValue::Gauge(value)
        | PrometheusValue::Untyped(value) => Some(*value),
        PrometheusValue::Histogram(_) | PrometheusValue::Summary(_) => None,
    }
}

fn metric_total(scrape: &Scrape, prefixes: &[&str]) -> Option<f64> {
    let values = scrape
        .samples
        .iter()
        .filter(|sample| {
            prefixes
                .iter()
                .any(|prefix| sample.metric.starts_with(prefix))
        })
        .filter_map(|sample| metric_value(&sample.value))
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.into_iter().sum())
}

fn parse_collector_metrics(text: &str) -> Result<CollectorMetrics, MaremmaError> {
    let scrape = Scrape::parse(
        text.lines()
            .map(|line| Ok::<String, io::Error>(line.to_string())),
    )
    .map_err(|error| {
        MaremmaError::Generic(format!("Failed to parse collector metrics: {error}"))
    })?;

    let mut capacities = HashMap::new();
    let mut sizes = HashMap::new();
    for sample in &scrape.samples {
        if !matches!(
            sample.metric.as_str(),
            "otelcol_exporter_queue_capacity" | "otelcol_exporter_queue_size"
        ) {
            continue;
        }
        let Some(value) = metric_value(&sample.value) else {
            continue;
        };
        let exporter = sample.labels.get("exporter").unwrap_or("unknown");
        let data_type = sample.labels.get("data_type").unwrap_or("unknown");
        let key = (exporter.to_string(), data_type.to_string());
        if sample.metric == "otelcol_exporter_queue_capacity" {
            capacities.insert(key, value);
        } else {
            sizes.insert(key, value);
        }
    }

    if capacities.is_empty() || sizes.is_empty() {
        return Err(MaremmaError::Generic(
            "Collector queue size or capacity metrics are missing".to_string(),
        ));
    }

    let mut max_queue_percent: f64 = 0.0;
    for (key, size) in sizes {
        let capacity = capacities.get(&key).ok_or_else(|| {
            MaremmaError::Generic(format!(
                "Collector queue capacity is missing for exporter={} data_type={}",
                key.0, key.1
            ))
        })?;
        if *capacity <= 0.0 {
            return Err(MaremmaError::Generic(format!(
                "Collector queue capacity is not positive for exporter={} data_type={}",
                key.0, key.1
            )));
        }
        max_queue_percent = max_queue_percent.max(size / capacity * 100.0);
    }

    let accepted = metric_total(&scrape, &["otelcol_receiver_accepted_"]).ok_or_else(|| {
        MaremmaError::Generic("Collector accepted-item metrics are missing".to_string())
    })?;
    let exported = metric_total(&scrape, &["otelcol_exporter_sent_"]).ok_or_else(|| {
        MaremmaError::Generic("Collector exported-item metrics are missing".to_string())
    })?;

    Ok(CollectorMetrics {
        max_queue_percent,
        accepted,
        exported,
        refused: metric_total(&scrape, &["otelcol_receiver_refused_"]).unwrap_or(0.0),
        enqueue_failed: metric_total(&scrape, &["otelcol_exporter_enqueue_failed_"]).unwrap_or(0.0),
        send_failed: metric_total(&scrape, &["otelcol_exporter_send_failed_"]).unwrap_or(0.0),
    })
}

fn evaluate_metrics(
    metrics: &CollectorMetrics,
    warning_percent: f64,
    critical_percent: f64,
) -> ServiceStatus {
    if metrics.max_queue_percent >= critical_percent {
        ServiceStatus::Critical
    } else if metrics.max_queue_percent >= warning_percent {
        ServiceStatus::Warning
    } else {
        ServiceStatus::Ok
    }
}

impl OtelCollectorService {
    async fn run_check(&self, client: &Client) -> Result<(String, ServiceStatus), MaremmaError> {
        let health_text = get_proxy_text(
            client,
            &self.namespace,
            &self.service_name,
            self.health_port,
            "/",
        )
        .await?;
        let health: HealthResponse = serde_json::from_str(&health_text).map_err(|error| {
            MaremmaError::Generic(format!("Invalid collector health response: {error}"))
        })?;
        if health.status != "Server available" {
            return Ok((
                format!("CRITICAL: collector health status={}", health.status),
                ServiceStatus::Critical,
            ));
        }

        let metrics_text = get_proxy_text(
            client,
            &self.namespace,
            &self.service_name,
            self.metrics_port,
            "/metrics",
        )
        .await?;
        let metrics = parse_collector_metrics(&metrics_text)?;
        let status = evaluate_metrics(
            &metrics,
            self.queue_warning_percent,
            self.queue_critical_percent,
        );
        Ok((
            format!(
                "{status}: health={} max_queue={:.1}% accepted={:.0} exported={:.0} refused={:.0} enqueue_failed={:.0} send_failed={:.0}",
                health.status,
                metrics.max_queue_percent,
                metrics.accepted,
                metrics.exported,
                metrics.refused,
                metrics.enqueue_failed,
                metrics.send_failed,
            ),
            status,
        ))
    }
}

#[async_trait]
impl ServiceTrait for OtelCollectorService {
    fn validate(&self) -> Result<(), MaremmaError> {
        if !valid_dns_label(&self.namespace) {
            return Err(MaremmaError::Configuration(
                "namespace must be a valid Kubernetes DNS label".to_string(),
            ));
        }
        if !valid_dns_label(&self.service_name) {
            return Err(MaremmaError::Configuration(
                "service_name must be a valid Kubernetes DNS label".to_string(),
            ));
        }
        if self.health_port == 0 || self.metrics_port == 0 {
            return Err(MaremmaError::Configuration(
                "collector ports must be greater than zero".to_string(),
            ));
        }
        if !(0.0..=100.0).contains(&self.queue_warning_percent)
            || !(0.0..=100.0).contains(&self.queue_critical_percent)
            || self.queue_warning_percent >= self.queue_critical_percent
        {
            return Err(MaremmaError::Configuration(
                "queue thresholds must satisfy 0 <= warning < critical <= 100".to_string(),
            ));
        }
        Ok(())
    }

    async fn run(
        &self,
        host: &entities::host::Model,
        _context: &CheckExecutionContext,
    ) -> Result<CheckResult, MaremmaError> {
        let start_time = Utc::now();
        let config = self.overlay_host_config(&self.get_host_config(&self.name, host)?)?;
        config.validate()?;
        let client = match Client::try_default().await {
            Ok(client) => client,
            Err(error) => {
                return Ok(CheckResult {
                    timestamp: start_time,
                    result_text: format!("UNKNOWN: Unable to configure Kubernetes client: {error}"),
                    status: ServiceStatus::Unknown,
                    time_elapsed: Utc::now() - start_time,
                });
            }
        };

        let (result_text, status) = match config.run_check(&client).await {
            Ok(result) => result,
            Err(error) => (format!("CRITICAL: {error:?}"), ServiceStatus::Critical),
        };
        Ok(CheckResult {
            timestamp: start_time,
            result_text,
            status,
            time_elapsed: Utc::now() - start_time,
        })
    }

    fn as_json_pretty(&self, host: &entities::host::Model) -> Result<String, MaremmaError> {
        let config = self.overlay_host_config(&self.get_host_config(&self.name, host)?)?;
        Ok(serde_json::to_string_pretty(&config)?)
    }

    fn jitter_value(&self) -> u32 {
        self.jitter.unwrap_or(0) as u32
    }
}

#[cfg(test)]
mod tests {
    use schemars::schema_for;

    use super::*;

    const BASE_METRICS: &str = r#"
# TYPE otelcol_exporter_queue_capacity gauge
otelcol_exporter_queue_capacity{data_type="logs",exporter="clickhouse"} 1000
otelcol_exporter_queue_capacity{data_type="traces",exporter="clickhouse"} 200
# TYPE otelcol_exporter_queue_size gauge
otelcol_exporter_queue_size{data_type="logs",exporter="clickhouse"} 0
otelcol_exporter_queue_size{data_type="traces",exporter="clickhouse"} 0
# TYPE otelcol_receiver_accepted_log_records counter
otelcol_receiver_accepted_log_records{receiver="otlp",transport="http"} 10
# TYPE otelcol_exporter_sent_log_records counter
otelcol_exporter_sent_log_records{exporter="clickhouse"} 9
otelcol_receiver_refused_log_records{receiver="otlp",transport="http"} 2
otelcol_exporter_enqueue_failed_log_records{exporter="clickhouse"} 3
otelcol_exporter_send_failed_log_records{exporter="clickhouse"} 4
"#;

    fn service() -> OtelCollectorService {
        OtelCollectorService {
            name: "collector".to_string(),
            namespace: "clickstack".to_string(),
            service_name: "clickstack-otel-collector".to_string(),
            health_port: DEFAULT_HEALTH_PORT,
            metrics_port: DEFAULT_METRICS_PORT,
            queue_warning_percent: DEFAULT_QUEUE_WARNING_PERCENT,
            queue_critical_percent: DEFAULT_QUEUE_CRITICAL_PERCENT,
            cron_schedule: "*/5 * * * *".parse().expect("valid cron schedule"),
            jitter: Some(10),
        }
    }

    fn with_queue(size: u16, capacity: u16) -> String {
        format!(
            "# TYPE otelcol_exporter_queue_capacity gauge\n\
             otelcol_exporter_queue_capacity{{data_type=\"logs\",exporter=\"clickhouse\"}} {capacity}\n\
             # TYPE otelcol_exporter_queue_size gauge\n\
             otelcol_exporter_queue_size{{data_type=\"logs\",exporter=\"clickhouse\"}} {size}\n\
             otelcol_receiver_accepted_log_records 10\n\
             otelcol_exporter_sent_log_records 10\n"
        )
    }

    #[test]
    fn parses_healthy_collector_response() {
        let health: HealthResponse = serde_json::from_str(r#"{"status":"Server available"}"#)
            .expect("health response should parse");
        assert_eq!(health.status, "Server available");
    }

    #[test]
    fn rejects_malformed_health_response() {
        assert!(serde_json::from_str::<HealthResponse>(r#"{"state":"ok"}"#).is_err());
    }

    #[test]
    fn parses_metrics_and_diagnostics() {
        let metrics = parse_collector_metrics(BASE_METRICS).expect("metrics should parse");
        assert_eq!(metrics.max_queue_percent, 0.0);
        assert_eq!(metrics.accepted, 10.0);
        assert_eq!(metrics.exported, 9.0);
        assert_eq!(metrics.refused, 2.0);
        assert_eq!(metrics.enqueue_failed, 3.0);
        assert_eq!(metrics.send_failed, 4.0);
    }

    #[test]
    fn uses_highest_queue_utilization_across_exporters() {
        let metrics = parse_collector_metrics(&format!(
            "{BASE_METRICS}\n\
             otelcol_exporter_queue_capacity{{data_type=\"logs\",exporter=\"otlp\"}} 200\n\
             otelcol_exporter_queue_size{{data_type=\"logs\",exporter=\"otlp\"}} 100\n"
        ))
        .expect("multiple exporter metrics should parse");
        assert_eq!(metrics.max_queue_percent, 50.0);
    }

    #[test]
    fn rejects_missing_required_metrics() {
        let error = parse_collector_metrics("otelcol_process_uptime 1")
            .expect_err("missing queue metrics should fail");
        assert!(format!("{error:?}").contains("queue size or capacity"));
    }

    #[test]
    fn accepts_present_zero_activity_metrics() {
        let metrics = parse_collector_metrics(
            &with_queue(0, 100)
                .replace("accepted_log_records 10", "accepted_log_records 0")
                .replace("sent_log_records 10", "sent_log_records 0"),
        )
        .expect("present metric families may report zero activity");
        assert_eq!(metrics.accepted, 0.0);
        assert_eq!(metrics.exported, 0.0);
    }

    #[test]
    fn evaluates_queue_threshold_boundaries() {
        for (size, expected) in [
            (49, ServiceStatus::Ok),
            (50, ServiceStatus::Warning),
            (80, ServiceStatus::Critical),
        ] {
            let metrics = parse_collector_metrics(&with_queue(size, 100))
                .expect("queue metrics should parse");
            assert_eq!(
                evaluate_metrics(
                    &metrics,
                    DEFAULT_QUEUE_WARNING_PERCENT,
                    DEFAULT_QUEUE_CRITICAL_PERCENT,
                ),
                expected
            );
        }
    }

    #[test]
    fn validates_configuration() {
        service().validate().expect("configuration should validate");

        let mut invalid = service();
        invalid.queue_warning_percent = 80.0;
        invalid.queue_critical_percent = 50.0;
        assert!(invalid.validate().is_err());

        invalid = service();
        invalid.namespace.clear();
        assert!(invalid.validate().is_err());

        invalid = service();
        invalid.health_port = 0;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn parses_public_service_configuration() {
        let value = json!({
            "name": "collector",
            "service_type": "otel_collector",
            "host_groups": ["k8s_leader"],
            "namespace": "clickstack",
            "service_name": "clickstack-otel-collector",
            "cron_schedule": "*/5 * * * *"
        });

        let service = Service::try_from(&value).expect("collector service should parse");
        assert_eq!(service.service_type, ServiceType::OtelCollector);
    }

    #[test]
    fn overlays_host_configuration() {
        let value = json!({
            "service_name": "alternate-collector",
            "metrics_port": 9999,
            "queue_warning_percent": 25.0
        });

        let overlaid = service()
            .overlay_host_config(value.as_object().expect("host config should be an object"))
            .expect("host configuration should overlay");
        assert_eq!(overlaid.service_name, "alternate-collector");
        assert_eq!(overlaid.metrics_port, 9999);
        assert_eq!(overlaid.queue_warning_percent, 25.0);
        assert_eq!(overlaid.health_port, DEFAULT_HEALTH_PORT);
    }

    #[test]
    fn schema_serializes_required_collector_fields() {
        let schema = serde_json::to_value(schema_for!(OtelCollectorService))
            .expect("collector schema should serialize");
        let required = schema
            .pointer("/required")
            .and_then(Json::as_array)
            .expect("collector schema should declare required fields");
        for field in ["name", "namespace", "service_name", "cron_schedule"] {
            assert!(required.contains(&Json::String(field.to_string())));
        }
    }
}
