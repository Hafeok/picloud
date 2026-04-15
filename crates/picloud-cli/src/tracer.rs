/// CLI OTel Tracer — every command produces a trace (FT-047, ADR-045)
///
/// Generates OTel-compatible SpanRecords for every CLI command execution.
/// The root span represents the CLI command itself; child spans represent
/// HTTP calls to the cluster. After execution, spans are flushed to the
/// cluster's `/otel` endpoint (best-effort — failure to flush does not
/// affect the command outcome).

use chrono::{DateTime, Utc};
use picloud_domain::events::SpanRecord;
use serde_json::json;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// 16-byte random hex string for OTel trace IDs (32 hex chars).
fn generate_trace_id() -> String {
    let id = Uuid::new_v4();
    id.as_simple().to_string()
}

/// 8-byte random hex string for OTel span IDs (16 hex chars).
fn generate_span_id() -> String {
    let id = Uuid::new_v4();
    // Use the first 16 hex chars (8 bytes) of a UUID
    id.as_simple().to_string()[..16].to_string()
}

/// A single in-flight span being timed.
#[derive(Debug, Clone)]
pub struct SpanBuilder {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub operation_name: String,
    pub service_name: String,
    pub start_time: DateTime<Utc>,
    pub attributes: serde_json::Value,
}

impl SpanBuilder {
    /// Finish the span and produce a completed SpanRecord.
    pub fn finish(self, status: &str) -> SpanRecord {
        let end_time = Utc::now();
        let duration_ms = (end_time - self.start_time)
            .num_milliseconds()
            .max(0) as u64;
        SpanRecord {
            trace_id: self.trace_id,
            span_id: self.span_id,
            parent_span_id: self.parent_span_id,
            operation_name: self.operation_name,
            service_name: self.service_name,
            start_time: self.start_time,
            end_time,
            duration_ms,
            status: status.to_string(),
            attributes: self.attributes,
        }
    }
}

/// Collects spans produced during a single CLI command invocation.
///
/// Thread-safe via interior `Mutex` so child spans can be added from
/// async tasks that share a reference.
#[derive(Debug, Clone)]
pub struct CliTracer {
    trace_id: String,
    root_span_id: String,
    service_name: String,
    collected: Arc<Mutex<Vec<SpanRecord>>>,
}

impl CliTracer {
    /// Create a new tracer for a CLI command.
    pub fn new(_command_name: &str) -> Self {
        Self {
            trace_id: generate_trace_id(),
            root_span_id: generate_span_id(),
            service_name: "picloud-cli".to_string(),
            collected: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// The W3C traceparent header value for the root span.
    /// Format: `00-{trace_id}-{span_id}-01`
    pub fn traceparent(&self) -> String {
        format!("00-{}-{}-01", self.trace_id, self.root_span_id)
    }

    /// The trace ID for this command invocation.
    pub fn trace_id(&self) -> &str {
        &self.trace_id
    }

    /// The root span ID for this command invocation.
    pub fn root_span_id(&self) -> &str {
        &self.root_span_id
    }

    /// Start the root span for the CLI command.
    pub fn start_root_span(&self, command_name: &str) -> SpanBuilder {
        SpanBuilder {
            trace_id: self.trace_id.clone(),
            span_id: self.root_span_id.clone(),
            parent_span_id: None,
            operation_name: format!("picloud {}", command_name),
            service_name: self.service_name.clone(),
            start_time: Utc::now(),
            attributes: json!({
                "cli.command": command_name,
                "service.name": "picloud-cli",
            }),
        }
    }

    /// Start a child span (e.g. for an HTTP request to the cluster).
    pub fn start_child_span(&self, operation_name: &str) -> SpanBuilder {
        SpanBuilder {
            trace_id: self.trace_id.clone(),
            span_id: generate_span_id(),
            parent_span_id: Some(self.root_span_id.clone()),
            operation_name: operation_name.to_string(),
            service_name: self.service_name.clone(),
            start_time: Utc::now(),
            attributes: json!({
                "span.kind": "client",
            }),
        }
    }

    /// Record a completed span.
    pub fn record(&self, span: SpanRecord) {
        let mut collected = self.collected.lock().unwrap();
        collected.push(span);
    }

    /// Finish the root span and record it.
    pub fn finish_root(&self, root: SpanBuilder, status: &str) {
        let span = root.finish(status);
        self.record(span);
    }

    /// Return all collected spans.
    pub fn spans(&self) -> Vec<SpanRecord> {
        self.collected.lock().unwrap().clone()
    }

    /// Flush all collected spans to the cluster's `/otel` endpoint.
    ///
    /// Best-effort — returns Ok(()) even if the flush fails, since we
    /// don't want tracing failures to break CLI commands.
    pub async fn flush(&self, base_url: &str, token: Option<&str>) {
        let spans = self.spans();
        if spans.is_empty() {
            return;
        }

        let otlp_payload = spans_to_otlp(&spans);

        let client = reqwest::Client::builder()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let mut request = client
            .post(format!("{}/otel", base_url))
            .header("Content-Type", "application/json")
            .json(&otlp_payload);

        if let Some(tok) = token {
            request = request.header("Authorization", format!("Bearer {}", tok));
        }

        // Fire and forget — don't let tracing failures affect the CLI
        let _ = request.send().await;
    }
}

/// Convert SpanRecords into the simplified OTLP JSON format
/// accepted by the platform's `/otel` endpoint.
pub fn spans_to_otlp(spans: &[SpanRecord]) -> serde_json::Value {
    let span_objects: Vec<serde_json::Value> = spans
        .iter()
        .map(|s| {
            let mut obj = json!({
                "traceId": s.trace_id,
                "spanId": s.span_id,
                "operationName": s.operation_name,
                "serviceName": s.service_name,
                "startTimeUnixNano": s.start_time.timestamp_nanos_opt().unwrap_or(0).to_string(),
                "endTimeUnixNano": s.end_time.timestamp_nanos_opt().unwrap_or(0).to_string(),
                "status": { "code": if s.status == "OK" { 1 } else { 2 } },
                "attributes": s.attributes,
            });
            if let Some(ref parent) = s.parent_span_id {
                obj.as_object_mut()
                    .unwrap()
                    .insert("parentSpanId".to_string(), json!(parent));
            }
            obj
        })
        .collect();

    json!({
        "spans": span_objects
    })
}

/// Extract a human-readable command name from the CLI subcommand for tracing.
pub fn command_name(args: &[String]) -> String {
    // Skip the binary name, collect subcommand words until we hit a flag
    args.iter()
        .skip(1)
        .take_while(|a| !a.starts_with('-'))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_id_is_32_hex_chars() {
        let id = generate_trace_id();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn span_id_is_16_hex_chars() {
        let id = generate_span_id();
        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn traceparent_format() {
        let tracer = CliTracer::new("cluster status");
        let tp = tracer.traceparent();
        let parts: Vec<&str> = tp.split('-').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "00");
        assert_eq!(parts[1].len(), 32); // trace_id
        assert_eq!(parts[2].len(), 16); // span_id
        assert_eq!(parts[3], "01");
    }

    #[test]
    fn root_span_has_no_parent() {
        let tracer = CliTracer::new("cluster status");
        let root = tracer.start_root_span("cluster status");
        assert!(root.parent_span_id.is_none());
        assert_eq!(root.span_id, tracer.root_span_id);
        assert_eq!(root.trace_id, tracer.trace_id);
    }

    #[test]
    fn child_span_references_root() {
        let tracer = CliTracer::new("cluster status");
        let child = tracer.start_child_span("HTTP GET /");
        assert_eq!(
            child.parent_span_id.as_deref(),
            Some(tracer.root_span_id())
        );
        assert_eq!(child.trace_id, tracer.trace_id);
        // child span_id should differ from root
        assert_ne!(child.span_id, tracer.root_span_id);
    }

    #[test]
    fn finish_produces_valid_span_record() {
        let tracer = CliTracer::new("cluster status");
        let root = tracer.start_root_span("cluster status");
        let span = root.finish("OK");
        assert_eq!(span.status, "OK");
        assert_eq!(span.service_name, "picloud-cli");
        assert!(span.operation_name.contains("cluster status"));
        assert!(span.end_time >= span.start_time);
    }

    #[test]
    fn collect_and_retrieve_spans() {
        let tracer = CliTracer::new("cluster status");

        // Root span
        let root = tracer.start_root_span("cluster status");
        let root_record = root.finish("OK");
        tracer.record(root_record);

        // Child span
        let child = tracer.start_child_span("HTTP GET /");
        let child_record = child.finish("OK");
        tracer.record(child_record);

        let spans = tracer.spans();
        assert_eq!(spans.len(), 2);
        // All spans share the same trace_id
        assert!(spans.iter().all(|s| s.trace_id == tracer.trace_id));
        // Root has no parent, child references root
        assert!(spans[0].parent_span_id.is_none());
        assert_eq!(
            spans[1].parent_span_id.as_deref(),
            Some(tracer.root_span_id())
        );
    }

    #[test]
    fn spans_to_otlp_produces_valid_json() {
        let tracer = CliTracer::new("cluster status");
        let root = tracer.start_root_span("cluster status");
        let span = root.finish("OK");
        tracer.record(span);

        let otlp = spans_to_otlp(&tracer.spans());
        let spans_arr = otlp.get("spans").unwrap().as_array().unwrap();
        assert_eq!(spans_arr.len(), 1);

        let s = &spans_arr[0];
        assert!(s.get("traceId").is_some());
        assert!(s.get("spanId").is_some());
        assert!(s.get("operationName").is_some());
        assert!(s.get("serviceName").is_some());
        assert!(s.get("startTimeUnixNano").is_some());
        assert!(s.get("endTimeUnixNano").is_some());
    }

    #[test]
    fn command_name_extraction() {
        let args = vec![
            "picloud".to_string(),
            "cluster".to_string(),
            "status".to_string(),
        ];
        assert_eq!(command_name(&args), "cluster status");

        let args_with_flags = vec![
            "picloud".to_string(),
            "resource".to_string(),
            "apply".to_string(),
            "--path".to_string(),
            "/tmp/foo".to_string(),
        ];
        assert_eq!(command_name(&args_with_flags), "resource apply");
    }
}
