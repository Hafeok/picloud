/// JSONL Telemetry Store (ADR-046)
///
/// Stores telemetry spans and metrics as JSON-lines files, partitioned hourly.
/// This is a lightweight implementation using the same approach as the event log.
/// The interface matches what a future Parquet/DataFusion backend would provide.
///
/// Directory layout:
///   {base_path}/
///     spans/
///       2026-04-07-14.jsonl   (one file per hour)
///       2026-04-07-15.jsonl
///     metrics/
///       2026-04-07-14.jsonl
///       2026-04-07-15.jsonl

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::events::{MetricRecord, SpanRecord, TelemetryFilter};
use picloud_domain::traits::TelemetryStore;

/// JSONL-backed telemetry store with hourly file partitioning.
pub struct JsonlTelemetryStore {
    base_path: PathBuf,
    /// Mutex to serialize writes (avoid interleaved lines)
    write_lock: Arc<Mutex<()>>,
    /// Retention period in hours (default: 168 = 7 days)
    retention_hours: u64,
}

impl JsonlTelemetryStore {
    /// Create a new JSONL telemetry store at the given path.
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        let base_path = base_path.into();
        // Ensure directories exist
        let _ = std::fs::create_dir_all(base_path.join("spans"));
        let _ = std::fs::create_dir_all(base_path.join("metrics"));

        Self {
            base_path,
            write_lock: Arc::new(Mutex::new(())),
            retention_hours: 168, // 7 days
        }
    }

    /// Set the retention period in hours.
    pub fn with_retention_hours(mut self, hours: u64) -> Self {
        self.retention_hours = hours;
        self
    }

    /// Compute the JSONL file path for a given signal and timestamp.
    fn file_path(&self, signal: &str, ts: &DateTime<Utc>) -> PathBuf {
        let partition = ts.format("%Y-%m-%d-%H").to_string();
        self.base_path.join(signal).join(format!("{}.jsonl", partition))
    }

    /// Append a line to a JSONL file (creates if it doesn't exist).
    fn append_line(&self, path: &Path, line: &str) -> Result<()> {
        use std::io::Write;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| PiCloudError::TelemetryWriteFailed {
                reason: format!("Failed to create directory {}: {}", parent.display(), e),
            })?;
        }

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| PiCloudError::TelemetryWriteFailed {
                reason: format!("Failed to open {}: {}", path.display(), e),
            })?;

        writeln!(file, "{}", line).map_err(|e| PiCloudError::TelemetryWriteFailed {
            reason: format!("Failed to write to {}: {}", path.display(), e),
        })?;

        Ok(())
    }

    /// Read all records from JSONL files within a time range for a given signal.
    fn read_range<T: serde::de::DeserializeOwned>(
        &self,
        signal: &str,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<T>> {
        let mut results = Vec::new();
        let signal_dir = self.base_path.join(signal);

        if !signal_dir.exists() {
            return Ok(results);
        }

        // Generate all hourly partition keys in the range
        let mut current = from
            .date_naive()
            .and_hms_opt(from.time().hour(), 0, 0)
            .unwrap();
        let to_naive = to.naive_utc();

        while current <= to_naive {
            let partition = current.format("%Y-%m-%d-%H").to_string();
            let path = signal_dir.join(format!("{}.jsonl", partition));

            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for line in content.lines() {
                        if line.trim().is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<T>(line) {
                            Ok(record) => results.push(record),
                            Err(e) => {
                                debug!(
                                    file = %path.display(),
                                    error = %e,
                                    "Skipping malformed JSONL line"
                                );
                            }
                        }
                    }
                }
            }

            current += chrono::Duration::hours(1);
        }

        Ok(results)
    }

    /// Start the retention cleanup loop as a background task.
    /// Runs every hour and deletes files older than the retention window.
    pub fn start_retention_cleanup(&self) -> tokio::task::JoinHandle<()> {
        let base_path = self.base_path.clone();
        let retention_hours = self.retention_hours;

        tokio::spawn(async move {
            let interval = tokio::time::Duration::from_secs(3600); // 1 hour
            info!(
                retention_hours = retention_hours,
                "Telemetry retention cleanup started"
            );

            loop {
                tokio::time::sleep(interval).await;

                let cutoff = Utc::now() - chrono::Duration::hours(retention_hours as i64);
                let cutoff_str = cutoff.format("%Y-%m-%d-%H").to_string();

                for signal in &["spans", "metrics"] {
                    let dir = base_path.join(signal);
                    if !dir.exists() {
                        continue;
                    }

                    match std::fs::read_dir(&dir) {
                        Ok(entries) => {
                            for entry in entries.flatten() {
                                let filename = entry.file_name();
                                let name = filename.to_string_lossy();
                                // Files are named like "2026-04-07-14.jsonl"
                                let partition = name.trim_end_matches(".jsonl");
                                if partition < cutoff_str.as_str() {
                                    if let Err(e) = std::fs::remove_file(entry.path()) {
                                        warn!(
                                            file = %entry.path().display(),
                                            error = %e,
                                            "Failed to remove expired telemetry file"
                                        );
                                    } else {
                                        debug!(
                                            file = %entry.path().display(),
                                            "Removed expired telemetry file"
                                        );
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!(
                                dir = %dir.display(),
                                error = %e,
                                "Failed to read telemetry directory for cleanup"
                            );
                        }
                    }
                }
            }
        })
    }
}

use chrono::Timelike;

#[async_trait]
impl TelemetryStore for JsonlTelemetryStore {
    async fn write_spans(&self, spans: Vec<SpanRecord>) -> Result<()> {
        let _lock = self.write_lock.lock().await;
        for span in &spans {
            let path = self.file_path("spans", &span.start_time);
            let line = serde_json::to_string(span).map_err(|e| PiCloudError::TelemetryWriteFailed {
                reason: format!("Failed to serialize span: {}", e),
            })?;
            self.append_line(&path, &line)?;
        }
        debug!(count = spans.len(), "Wrote spans to JSONL store");
        Ok(())
    }

    async fn write_metrics(&self, metrics: Vec<MetricRecord>) -> Result<()> {
        let _lock = self.write_lock.lock().await;
        for metric in &metrics {
            let path = self.file_path("metrics", &metric.timestamp);
            let line = serde_json::to_string(metric).map_err(|e| {
                PiCloudError::TelemetryWriteFailed {
                    reason: format!("Failed to serialize metric: {}", e),
                }
            })?;
            self.append_line(&path, &line)?;
        }
        debug!(count = metrics.len(), "Wrote metrics to JSONL store");
        Ok(())
    }

    async fn query_spans(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        filter: TelemetryFilter,
    ) -> Result<Vec<SpanRecord>> {
        let all: Vec<SpanRecord> = self.read_range("spans", from, to)?;

        let filtered = all
            .into_iter()
            .filter(|s| {
                // Time range filter
                if s.start_time < from || s.start_time > to {
                    return false;
                }
                // Service name filter
                if let Some(ref svc) = filter.service_name {
                    if &s.service_name != svc {
                        return false;
                    }
                }
                // Operation name filter
                if let Some(ref op) = filter.operation_name {
                    if &s.operation_name != op {
                        return false;
                    }
                }
                // Minimum duration filter
                if let Some(min_dur) = filter.min_duration_ms {
                    if s.duration_ms < min_dur {
                        return false;
                    }
                }
                true
            })
            .collect();

        Ok(filtered)
    }

    async fn query_metrics(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        filter: TelemetryFilter,
    ) -> Result<Vec<MetricRecord>> {
        let all: Vec<MetricRecord> = self.read_range("metrics", from, to)?;

        let filtered = all
            .into_iter()
            .filter(|m| {
                // Time range filter
                if m.timestamp < from || m.timestamp > to {
                    return false;
                }
                // Service name filter
                if let Some(ref svc) = filter.service_name {
                    if &m.service_name != svc {
                        return false;
                    }
                }
                // Metric name filter
                if let Some(ref name) = filter.metric_name {
                    if &m.name != name {
                        return false;
                    }
                }
                true
            })
            .collect();

        Ok(filtered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;

    fn temp_store() -> (JsonlTelemetryStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("picloud-telemetry-test-{}", uuid::Uuid::new_v4()));
        let store = JsonlTelemetryStore::new(&dir);
        (store, dir)
    }

    #[tokio::test]
    async fn write_and_query_spans() {
        let (store, dir) = temp_store();
        let now = Utc::now();

        let spans = vec![
            SpanRecord {
                trace_id: "t1".to_string(),
                span_id: "s1".to_string(),
                parent_span_id: None,
                operation_name: "GET /api".to_string(),
                service_name: "api-server".to_string(),
                start_time: now,
                end_time: now + chrono::Duration::milliseconds(100),
                duration_ms: 100,
                status: "OK".to_string(),
                attributes: serde_json::json!({}),
            },
            SpanRecord {
                trace_id: "t2".to_string(),
                span_id: "s2".to_string(),
                parent_span_id: None,
                operation_name: "POST /orders".to_string(),
                service_name: "order-svc".to_string(),
                start_time: now,
                end_time: now + chrono::Duration::milliseconds(200),
                duration_ms: 200,
                status: "OK".to_string(),
                attributes: serde_json::json!({}),
            },
        ];

        store.write_spans(spans).await.unwrap();

        // Query all
        let result = store
            .query_spans(
                now - chrono::Duration::minutes(1),
                now + chrono::Duration::minutes(1),
                TelemetryFilter::default(),
            )
            .await
            .unwrap();
        assert_eq!(result.len(), 2);

        // Query by service name
        let result = store
            .query_spans(
                now - chrono::Duration::minutes(1),
                now + chrono::Duration::minutes(1),
                TelemetryFilter {
                    service_name: Some("api-server".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].trace_id, "t1");

        // Query by min duration
        let result = store
            .query_spans(
                now - chrono::Duration::minutes(1),
                now + chrono::Duration::minutes(1),
                TelemetryFilter {
                    min_duration_ms: Some(150),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].trace_id, "t2");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn write_and_query_metrics() {
        let (store, dir) = temp_store();
        let now = Utc::now();

        let metrics = vec![MetricRecord {
            name: "http_latency_ms".to_string(),
            value: 42.5,
            unit: "ms".to_string(),
            metric_type: "gauge".to_string(),
            service_name: "api-server".to_string(),
            timestamp: now,
            attributes: serde_json::json!({}),
        }];

        store.write_metrics(metrics).await.unwrap();

        let result = store
            .query_metrics(
                now - chrono::Duration::minutes(1),
                now + chrono::Duration::minutes(1),
                TelemetryFilter::default(),
            )
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "http_latency_ms");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn empty_query_returns_empty() {
        let (store, dir) = temp_store();
        let now = Utc::now();

        let result = store
            .query_spans(
                now - chrono::Duration::minutes(1),
                now + chrono::Duration::minutes(1),
                TelemetryFilter::default(),
            )
            .await
            .unwrap();
        assert!(result.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
