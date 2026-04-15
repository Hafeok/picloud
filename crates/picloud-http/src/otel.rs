/// OpenTelemetry Stream and Aggregator (ADR-045)
///
/// Provides an in-process pub/sub channel for OTLP telemetry data.
/// Not Raft-replicated, not persistent — designed for high-throughput local streaming.
///
/// Subscribers:
/// - TelemetryStore (ADR-046): writes spans/metrics to JSONL files
/// - Aggregator: computes summaries every 15s, emits MetricRecorded events
/// - (Future) External exporter: forwards to Jaeger/Prometheus/etc.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use picloud_domain::events::{
    EventEnvelope, MetricEntry, MetricRecord, MetricRecordedPayload, SpanRecord,
    TelemetryAggregatedPayload,
};
use picloud_domain::iri::{IriBuilder, ResourceIri};
use picloud_domain::traits::{EventLog, TelemetryStore};

/// A telemetry datum flowing through the OTel stream.
#[derive(Debug, Clone)]
pub enum OtelDatum {
    Span(SpanRecord),
    Metric(MetricRecord),
    Log(OtelLogRecord),
}

/// A simplified log record from OTLP.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OtelLogRecord {
    pub timestamp: chrono::DateTime<Utc>,
    pub severity: String,
    pub body: String,
    pub service_name: String,
    pub attributes: serde_json::Value,
}

/// In-process pub/sub channel for OTel data.
/// Uses a tokio broadcast channel with a bounded buffer.
pub struct OtelStream {
    sender: broadcast::Sender<OtelDatum>,
}

impl OtelStream {
    /// Create a new OTel stream with the given buffer capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publish a datum to the stream.
    pub fn publish(&self, datum: OtelDatum) {
        // Ignore send errors (no subscribers = data is dropped, which is fine)
        let _ = self.sender.send(datum);
    }

    /// Subscribe to the stream. Returns a receiver that gets all future data.
    pub fn subscribe(&self) -> broadcast::Receiver<OtelDatum> {
        self.sender.subscribe()
    }

    /// Publish a batch of spans.
    pub fn publish_spans(&self, spans: &[SpanRecord]) {
        for span in spans {
            self.publish(OtelDatum::Span(span.clone()));
        }
    }

    /// Publish a batch of metrics.
    pub fn publish_metrics(&self, metrics: &[MetricRecord]) {
        for metric in metrics {
            self.publish(OtelDatum::Metric(metric.clone()));
        }
    }
}

/// The OTel aggregator subscribes to the OtelStream and every `interval_secs`
/// computes summary metrics, then emits a TelemetryAggregated event to the
/// platform event log (which feeds the RDF/alerts pipeline).
pub struct OtelAggregator {
    otel_stream: Arc<OtelStream>,
    event_log: Arc<dyn EventLog>,
    telemetry_store: Arc<dyn TelemetryStore>,
    iri_builder: IriBuilder,
    node_iri: ResourceIri,
}

impl OtelAggregator {
    pub fn new(
        otel_stream: Arc<OtelStream>,
        event_log: Arc<dyn EventLog>,
        telemetry_store: Arc<dyn TelemetryStore>,
        iri_builder: IriBuilder,
        node_iri: ResourceIri,
    ) -> Self {
        Self {
            otel_stream,
            event_log,
            telemetry_store,
            iri_builder,
            node_iri,
        }
    }

    /// Start the aggregation loop as a background task.
    pub fn start(self, interval_secs: u64) -> tokio::task::JoinHandle<()> {
        let mut rx = self.otel_stream.subscribe();
        let span_count = Arc::new(AtomicU64::new(0));
        let metric_count = Arc::new(AtomicU64::new(0));
        let log_count = Arc::new(AtomicU64::new(0));

        // Shared buffer for OTel metric data points — drained every aggregation interval
        let metric_buffer: Arc<Mutex<Vec<MetricRecord>>> = Arc::new(Mutex::new(Vec::new()));

        // Collector task: receives data from stream, writes to store, increments counters
        let span_count_c = span_count.clone();
        let metric_count_c = metric_count.clone();
        let log_count_c = log_count.clone();
        let metric_buffer_c = metric_buffer.clone();
        let store = self.telemetry_store.clone();

        tokio::spawn(async move {
            let mut span_batch: Vec<SpanRecord> = Vec::new();
            let mut metric_batch: Vec<MetricRecord> = Vec::new();
            let flush_interval = tokio::time::Duration::from_secs(5);
            let mut flush_timer = tokio::time::interval(flush_interval);

            loop {
                tokio::select! {
                    result = rx.recv() => {
                        match result {
                            Ok(datum) => {
                                match datum {
                                    OtelDatum::Span(span) => {
                                        span_count_c.fetch_add(1, Ordering::Relaxed);
                                        span_batch.push(span);
                                    }
                                    OtelDatum::Metric(metric) => {
                                        metric_count_c.fetch_add(1, Ordering::Relaxed);
                                        // Buffer metric data for aggregation into MetricRecorded events
                                        metric_buffer_c.lock().await.push(metric.clone());
                                        metric_batch.push(metric);
                                    }
                                    OtelDatum::Log(_) => {
                                        log_count_c.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                warn!(skipped = n, "OTel collector lagged");
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                info!("OTel stream closed — stopping collector");
                                break;
                            }
                        }
                    }
                    _ = flush_timer.tick() => {
                        // Flush accumulated batches to the store
                        if !span_batch.is_empty() {
                            let batch = std::mem::take(&mut span_batch);
                            if let Err(e) = store.write_spans(batch).await {
                                error!("Failed to write spans to telemetry store: {e}");
                            }
                        }
                        if !metric_batch.is_empty() {
                            let batch = std::mem::take(&mut metric_batch);
                            if let Err(e) = store.write_metrics(batch).await {
                                error!("Failed to write metrics to telemetry store: {e}");
                            }
                        }
                    }
                }
            }
        });

        // Aggregator task: every interval_secs, emit summary events
        let event_log = self.event_log;
        let iri_builder = self.iri_builder;
        let node_iri = self.node_iri;

        tokio::spawn(async move {
            let interval = tokio::time::Duration::from_secs(interval_secs);
            info!(
                interval_secs = interval_secs,
                "OTel aggregator started"
            );

            loop {
                tokio::time::sleep(interval).await;

                let spans = span_count.swap(0, Ordering::Relaxed);
                let metrics = metric_count.swap(0, Ordering::Relaxed);
                let logs = log_count.swap(0, Ordering::Relaxed);

                // Drain the metric buffer and aggregate into MetricRecorded events
                let buffered_metrics = {
                    let mut buf = metric_buffer.lock().await;
                    std::mem::take(&mut *buf)
                };

                // Aggregate OTel metrics: compute latest value per (name, unit) pair
                if !buffered_metrics.is_empty() {
                    let aggregated = aggregate_otel_metrics(&buffered_metrics);
                    let payload = MetricRecordedPayload {
                        node_iri: node_iri.clone(),
                        metrics: aggregated.clone(),
                    };

                    let event = EventEnvelope::new(
                        iri_builder.event_schema("MetricRecorded", 1),
                        "MetricRecorded",
                        node_iri.clone(),
                        None,
                        Uuid::new_v4(),
                        serde_json::to_value(&payload).unwrap_or_default(),
                    );

                    if let Err(e) = event_log.append(event).await {
                        error!("Failed to emit MetricRecorded event from OTel aggregator: {e}");
                    } else {
                        debug!(
                            metric_count = aggregated.len(),
                            "MetricRecorded event emitted from OTel aggregator"
                        );
                    }
                }

                if spans == 0 && metrics == 0 && logs == 0 {
                    debug!("No telemetry data in this window — skipping TelemetryAggregated");
                    continue;
                }

                let summaries = vec![
                    MetricEntry {
                        name: "otel_spans_received".to_string(),
                        value: spans as f64,
                        unit: "count".to_string(),
                    },
                    MetricEntry {
                        name: "otel_metrics_received".to_string(),
                        value: metrics as f64,
                        unit: "count".to_string(),
                    },
                    MetricEntry {
                        name: "otel_logs_received".to_string(),
                        value: logs as f64,
                        unit: "count".to_string(),
                    },
                ];

                let payload = TelemetryAggregatedPayload {
                    node_iri: node_iri.clone(),
                    span_count: spans,
                    metric_count: metrics,
                    log_count: logs,
                    summaries,
                };

                let event = EventEnvelope::new(
                    iri_builder.event_schema("TelemetryAggregated", 1),
                    "TelemetryAggregated",
                    node_iri.clone(),
                    None,
                    Uuid::new_v4(),
                    serde_json::to_value(&payload).unwrap_or_default(),
                );

                if let Err(e) = event_log.append(event).await {
                    error!("Failed to emit TelemetryAggregated event: {e}");
                } else {
                    debug!(
                        spans = spans,
                        metrics = metrics,
                        logs = logs,
                        "TelemetryAggregated event emitted"
                    );
                }
            }
        })
    }
}

/// Aggregate OTel metric data points into MetricEntry values.
///
/// For each unique metric name, computes the mean of all values received
/// in the aggregation window. Preserves the unit from the first occurrence.
/// This converts workload-reported OTel metrics into platform MetricRecorded
/// events that feed the RDF graph and alert pipeline.
pub fn aggregate_otel_metrics(records: &[MetricRecord]) -> Vec<MetricEntry> {
    // Accumulate (sum, count, unit) per metric name
    let mut accum: HashMap<String, (f64, u64, String)> = HashMap::new();

    for record in records {
        let entry = accum
            .entry(record.name.clone())
            .or_insert((0.0, 0, record.unit.clone()));
        entry.0 += record.value;
        entry.1 += 1;
    }

    let mut entries: Vec<MetricEntry> = accum
        .into_iter()
        .map(|(name, (sum, count, unit))| MetricEntry {
            name,
            value: sum / count as f64,
            unit,
        })
        .collect();

    // Sort for deterministic output
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// Parse simplified OTLP JSON into OtelDatum items.
/// Accepts a JSON body with optional "spans", "metrics", and "logs" arrays,
/// and also the standard OTLP format with "resourceSpans".
pub fn parse_otlp_json(body: &serde_json::Value) -> Vec<OtelDatum> {
    let mut data = Vec::new();

    // Parse spans (simplified format)
    if let Some(spans) = body.get("spans").and_then(|v| v.as_array()) {
        for span_json in spans {
            if let Ok(span) = serde_json::from_value::<SpanRecord>(span_json.clone()) {
                data.push(OtelDatum::Span(span));
            } else {
                warn!("Failed to parse span record from OTLP JSON");
            }
        }
    }

    // Parse standard OTLP format: resourceSpans -> scopeSpans -> spans
    if let Some(resource_spans) = body.get("resourceSpans").and_then(|v| v.as_array()) {
        for rs in resource_spans {
            // Extract service.name from resource attributes
            let service_name = rs
                .pointer("/resource/attributes")
                .and_then(|a| a.as_array())
                .and_then(|attrs| {
                    attrs.iter().find_map(|attr| {
                        let key = attr.get("key")?.as_str()?;
                        if key == "service.name" {
                            attr.pointer("/value/stringValue").and_then(|v| v.as_str()).map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or_else(|| "unknown".to_string());

            // Extract picloud.platform_version from resource attributes
            let platform_version = rs
                .pointer("/resource/attributes")
                .and_then(|a| a.as_array())
                .and_then(|attrs| {
                    attrs.iter().find_map(|attr| {
                        let key = attr.get("key")?.as_str()?;
                        if key == "picloud.platform_version" {
                            attr.pointer("/value/stringValue").and_then(|v| v.as_str()).map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                });

            if let Some(scope_spans) = rs.get("scopeSpans").and_then(|v| v.as_array()) {
                for ss in scope_spans {
                    if let Some(spans) = ss.get("spans").and_then(|v| v.as_array()) {
                        for span_json in spans {
                            let trace_id = span_json.get("traceId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let span_id = span_json.get("spanId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let parent_span_id = span_json.get("parentSpanId").and_then(|v| v.as_str()).map(|s| s.to_string());
                            let name = span_json.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let start_ns: u64 = span_json.get("startTimeUnixNano")
                                .and_then(|v| v.as_str().or_else(|| v.as_u64().map(|_| "")).and_then(|s| s.parse().ok()).or_else(|| v.as_u64()))
                                .unwrap_or(0);
                            let end_ns: u64 = span_json.get("endTimeUnixNano")
                                .and_then(|v| v.as_str().or_else(|| v.as_u64().map(|_| "")).and_then(|s| s.parse().ok()).or_else(|| v.as_u64()))
                                .unwrap_or(0);
                            let duration_ms = (end_ns.saturating_sub(start_ns)) / 1_000_000;

                            // Parse span-level attributes into a JSON map
                            let mut attrs = serde_json::Map::new();
                            if let Some(span_attrs) = span_json.get("attributes").and_then(|v| v.as_array()) {
                                for attr in span_attrs {
                                    if let (Some(key), Some(val)) = (
                                        attr.get("key").and_then(|v| v.as_str()),
                                        attr.pointer("/value/stringValue").and_then(|v| v.as_str()),
                                    ) {
                                        attrs.insert(key.to_string(), serde_json::Value::String(val.to_string()));
                                    }
                                }
                            }
                            // Also include resource-level attributes
                            if let Some(ref ver) = platform_version {
                                attrs.insert("picloud.platform_version".to_string(), serde_json::Value::String(ver.clone()));
                            }

                            let start_time = chrono::DateTime::from_timestamp(
                                (start_ns / 1_000_000_000) as i64,
                                (start_ns % 1_000_000_000) as u32,
                            ).unwrap_or_default();
                            let end_time = chrono::DateTime::from_timestamp(
                                (end_ns / 1_000_000_000) as i64,
                                (end_ns % 1_000_000_000) as u32,
                            ).unwrap_or_default();

                            let span = SpanRecord {
                                trace_id,
                                span_id,
                                parent_span_id,
                                operation_name: name,
                                service_name: service_name.clone(),
                                start_time,
                                end_time,
                                duration_ms,
                                status: "OK".to_string(),
                                attributes: serde_json::Value::Object(attrs),
                            };
                            data.push(OtelDatum::Span(span));
                        }
                    }
                }
            }
        }
    }

    // Parse metrics
    if let Some(metrics) = body.get("metrics").and_then(|v| v.as_array()) {
        for metric_json in metrics {
            if let Ok(metric) = serde_json::from_value::<MetricRecord>(metric_json.clone()) {
                data.push(OtelDatum::Metric(metric));
            } else {
                warn!("Failed to parse metric record from OTLP JSON");
            }
        }
    }

    // Parse logs
    if let Some(logs) = body.get("logs").and_then(|v| v.as_array()) {
        for log_json in logs {
            if let Ok(log) = serde_json::from_value::<OtelLogRecord>(log_json.clone()) {
                data.push(OtelDatum::Log(log));
            } else {
                warn!("Failed to parse log record from OTLP JSON");
            }
        }
    }

    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn otel_stream_publish_subscribe() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let stream = OtelStream::new(16);
            let mut rx = stream.subscribe();

            let span = SpanRecord {
                trace_id: "abc123".to_string(),
                span_id: "span1".to_string(),
                parent_span_id: None,
                operation_name: "GET /api".to_string(),
                service_name: "api-server".to_string(),
                start_time: Utc::now(),
                end_time: Utc::now(),
                duration_ms: 42,
                status: "OK".to_string(),
                attributes: serde_json::json!({}),
            };

            stream.publish(OtelDatum::Span(span.clone()));

            let received = rx.recv().await.unwrap();
            match received {
                OtelDatum::Span(s) => {
                    assert_eq!(s.trace_id, "abc123");
                    assert_eq!(s.operation_name, "GET /api");
                }
                _ => panic!("Expected Span"),
            }
        });
    }

    #[test]
    fn parse_otlp_json_spans() {
        let body = serde_json::json!({
            "spans": [{
                "trace_id": "t1",
                "span_id": "s1",
                "parent_span_id": null,
                "operation_name": "POST /orders",
                "service_name": "order-svc",
                "start_time": "2026-01-01T00:00:00Z",
                "end_time": "2026-01-01T00:00:01Z",
                "duration_ms": 1000,
                "status": "OK",
                "attributes": {}
            }]
        });

        let data = parse_otlp_json(&body);
        assert_eq!(data.len(), 1);
        match &data[0] {
            OtelDatum::Span(s) => {
                assert_eq!(s.trace_id, "t1");
                assert_eq!(s.service_name, "order-svc");
            }
            _ => panic!("Expected Span"),
        }
    }

    #[test]
    fn parse_otlp_json_metrics() {
        let body = serde_json::json!({
            "metrics": [{
                "name": "http_request_duration_ms",
                "value": 42.5,
                "unit": "ms",
                "metric_type": "gauge",
                "service_name": "api-server",
                "timestamp": "2026-01-01T00:00:00Z",
                "attributes": {"method": "GET"}
            }]
        });

        let data = parse_otlp_json(&body);
        assert_eq!(data.len(), 1);
        match &data[0] {
            OtelDatum::Metric(m) => {
                assert_eq!(m.name, "http_request_duration_ms");
                assert_eq!(m.service_name, "api-server");
            }
            _ => panic!("Expected Metric"),
        }
    }

    #[test]
    fn parse_otlp_json_empty() {
        let body = serde_json::json!({});
        let data = parse_otlp_json(&body);
        assert!(data.is_empty());
    }

    #[test]
    fn parse_otlp_json_mixed() {
        let body = serde_json::json!({
            "spans": [{
                "trace_id": "t1",
                "span_id": "s1",
                "parent_span_id": null,
                "operation_name": "op",
                "service_name": "svc",
                "start_time": "2026-01-01T00:00:00Z",
                "end_time": "2026-01-01T00:00:01Z",
                "duration_ms": 100,
                "status": "OK",
                "attributes": {}
            }],
            "metrics": [{
                "name": "m1",
                "value": 1.0,
                "unit": "count",
                "metric_type": "counter",
                "service_name": "svc",
                "timestamp": "2026-01-01T00:00:00Z",
                "attributes": {}
            }],
            "logs": [{
                "timestamp": "2026-01-01T00:00:00Z",
                "severity": "INFO",
                "body": "hello",
                "service_name": "svc",
                "attributes": {}
            }]
        });

        let data = parse_otlp_json(&body);
        assert_eq!(data.len(), 3);
    }

    #[test]
    fn aggregate_otel_metrics_single_metric() {
        let records = vec![MetricRecord {
            name: "http_request_duration_ms".to_string(),
            value: 42.0,
            unit: "ms".to_string(),
            metric_type: "gauge".to_string(),
            service_name: "api-server".to_string(),
            timestamp: Utc::now(),
            attributes: serde_json::json!({}),
        }];

        let aggregated = aggregate_otel_metrics(&records);
        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].name, "http_request_duration_ms");
        assert_eq!(aggregated[0].value, 42.0);
        assert_eq!(aggregated[0].unit, "ms");
    }

    #[test]
    fn aggregate_otel_metrics_computes_mean() {
        let records = vec![
            MetricRecord {
                name: "cpu_usage".to_string(),
                value: 20.0,
                unit: "percent".to_string(),
                metric_type: "gauge".to_string(),
                service_name: "svc".to_string(),
                timestamp: Utc::now(),
                attributes: serde_json::json!({}),
            },
            MetricRecord {
                name: "cpu_usage".to_string(),
                value: 40.0,
                unit: "percent".to_string(),
                metric_type: "gauge".to_string(),
                service_name: "svc".to_string(),
                timestamp: Utc::now(),
                attributes: serde_json::json!({}),
            },
            MetricRecord {
                name: "cpu_usage".to_string(),
                value: 60.0,
                unit: "percent".to_string(),
                metric_type: "gauge".to_string(),
                service_name: "svc".to_string(),
                timestamp: Utc::now(),
                attributes: serde_json::json!({}),
            },
        ];

        let aggregated = aggregate_otel_metrics(&records);
        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].name, "cpu_usage");
        assert!((aggregated[0].value - 40.0).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_otel_metrics_multiple_names() {
        let records = vec![
            MetricRecord {
                name: "alpha".to_string(),
                value: 10.0,
                unit: "ms".to_string(),
                metric_type: "gauge".to_string(),
                service_name: "svc".to_string(),
                timestamp: Utc::now(),
                attributes: serde_json::json!({}),
            },
            MetricRecord {
                name: "beta".to_string(),
                value: 20.0,
                unit: "count".to_string(),
                metric_type: "counter".to_string(),
                service_name: "svc".to_string(),
                timestamp: Utc::now(),
                attributes: serde_json::json!({}),
            },
        ];

        let aggregated = aggregate_otel_metrics(&records);
        assert_eq!(aggregated.len(), 2);
        // Sorted alphabetically
        assert_eq!(aggregated[0].name, "alpha");
        assert_eq!(aggregated[1].name, "beta");
    }

    #[test]
    fn aggregate_otel_metrics_empty() {
        let aggregated = aggregate_otel_metrics(&[]);
        assert!(aggregated.is_empty());
    }
}
