/// Parquet Telemetry Store (ADR-046)
///
/// Stores telemetry spans and metrics as Apache Parquet files, partitioned hourly.
/// This is the columnar storage backend described in ADR-046, replacing the
/// JSONL store with efficient, self-describing Parquet files on each node's NVMe.
///
/// Directory layout (per ADR-046):
///   {base_path}/
///     traces/
///       2025-07-01T00/        <- hourly partition
///         part-0001.parquet
///       2025-07-01T01/
///         part-0001.parquet
///     metrics/
///       2025-07-01T00/
///         part-0001.parquet

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use arrow::array::{Array, Float64Array, StringArray, UInt64Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::events::{MetricRecord, SpanRecord, TelemetryFilter};
use picloud_domain::traits::TelemetryStore;

// ---------------------------------------------------------------------------
// Parquet schemas — matching ADR-046 exactly
// ---------------------------------------------------------------------------

/// Arrow schema for trace spans (ADR-046 trace schema).
fn traces_schema() -> Schema {
    Schema::new(vec![
        Field::new("trace_id", DataType::Utf8, false),
        Field::new("span_id", DataType::Utf8, false),
        Field::new("parent_span_id", DataType::Utf8, true),
        Field::new("operation_name", DataType::Utf8, false),
        Field::new("service_name", DataType::Utf8, false),
        Field::new("start_time", DataType::Utf8, false),
        Field::new("end_time", DataType::Utf8, false),
        Field::new("duration_ms", DataType::UInt64, false),
        Field::new("status", DataType::Utf8, false),
        Field::new("attributes", DataType::Utf8, false),
    ])
}

/// Arrow schema for metric records (ADR-046 metric schema).
fn metrics_schema() -> Schema {
    Schema::new(vec![
        Field::new("name", DataType::Utf8, false),
        Field::new("value", DataType::Float64, false),
        Field::new("unit", DataType::Utf8, false),
        Field::new("metric_type", DataType::Utf8, false),
        Field::new("service_name", DataType::Utf8, false),
        Field::new("timestamp", DataType::Utf8, false),
        Field::new("attributes", DataType::Utf8, false),
    ])
}

// ---------------------------------------------------------------------------
// RecordBatch builders
// ---------------------------------------------------------------------------

/// Convert a batch of SpanRecords into an Arrow RecordBatch.
fn spans_to_record_batch(spans: &[SpanRecord]) -> Result<RecordBatch> {
    let schema = Arc::new(traces_schema());

    let trace_ids: Vec<&str> = spans.iter().map(|s| s.trace_id.as_str()).collect();
    let span_ids: Vec<&str> = spans.iter().map(|s| s.span_id.as_str()).collect();
    let parent_ids: Vec<Option<&str>> = spans
        .iter()
        .map(|s| s.parent_span_id.as_deref())
        .collect();
    let operations: Vec<&str> = spans.iter().map(|s| s.operation_name.as_str()).collect();
    let services: Vec<&str> = spans.iter().map(|s| s.service_name.as_str()).collect();
    let start_times: Vec<String> = spans
        .iter()
        .map(|s| s.start_time.to_rfc3339())
        .collect();
    let end_times: Vec<String> = spans.iter().map(|s| s.end_time.to_rfc3339()).collect();
    let durations: Vec<u64> = spans.iter().map(|s| s.duration_ms).collect();
    let statuses: Vec<&str> = spans.iter().map(|s| s.status.as_str()).collect();
    let attrs: Vec<String> = spans
        .iter()
        .map(|s| serde_json::to_string(&s.attributes).unwrap_or_else(|_| "{}".to_string()))
        .collect();

    let start_refs: Vec<&str> = start_times.iter().map(|s| s.as_str()).collect();
    let end_refs: Vec<&str> = end_times.iter().map(|s| s.as_str()).collect();
    let attr_refs: Vec<&str> = attrs.iter().map(|s| s.as_str()).collect();

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(trace_ids)),
            Arc::new(StringArray::from(span_ids)),
            Arc::new(StringArray::from(parent_ids)),
            Arc::new(StringArray::from(operations)),
            Arc::new(StringArray::from(services)),
            Arc::new(StringArray::from(start_refs)),
            Arc::new(StringArray::from(end_refs)),
            Arc::new(UInt64Array::from(durations)),
            Arc::new(StringArray::from(statuses)),
            Arc::new(StringArray::from(attr_refs)),
        ],
    )
    .map_err(|e| PiCloudError::TelemetryWriteFailed {
        reason: format!("Failed to build trace RecordBatch: {e}"),
    })
}

/// Convert a batch of MetricRecords into an Arrow RecordBatch.
fn metrics_to_record_batch(metrics: &[MetricRecord]) -> Result<RecordBatch> {
    let schema = Arc::new(metrics_schema());

    let names: Vec<&str> = metrics.iter().map(|m| m.name.as_str()).collect();
    let values: Vec<f64> = metrics.iter().map(|m| m.value).collect();
    let units: Vec<&str> = metrics.iter().map(|m| m.unit.as_str()).collect();
    let types: Vec<&str> = metrics.iter().map(|m| m.metric_type.as_str()).collect();
    let services: Vec<&str> = metrics.iter().map(|m| m.service_name.as_str()).collect();
    let timestamps: Vec<String> = metrics.iter().map(|m| m.timestamp.to_rfc3339()).collect();
    let attrs: Vec<String> = metrics
        .iter()
        .map(|m| serde_json::to_string(&m.attributes).unwrap_or_else(|_| "{}".to_string()))
        .collect();

    let ts_refs: Vec<&str> = timestamps.iter().map(|s| s.as_str()).collect();
    let attr_refs: Vec<&str> = attrs.iter().map(|s| s.as_str()).collect();

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(names)),
            Arc::new(Float64Array::from(values)),
            Arc::new(StringArray::from(units)),
            Arc::new(StringArray::from(types)),
            Arc::new(StringArray::from(services)),
            Arc::new(StringArray::from(ts_refs)),
            Arc::new(StringArray::from(attr_refs)),
        ],
    )
    .map_err(|e| PiCloudError::TelemetryWriteFailed {
        reason: format!("Failed to build metric RecordBatch: {e}"),
    })
}

// ---------------------------------------------------------------------------
// Read helpers
// ---------------------------------------------------------------------------

/// Read all SpanRecords from a single Parquet file.
fn read_spans_from_parquet(path: &Path) -> Result<Vec<SpanRecord>> {
    let file = std::fs::File::open(path).map_err(|e| PiCloudError::TelemetryQueryFailed {
        reason: format!("Failed to open {}: {e}", path.display()),
    })?;

    let reader =
        ParquetRecordBatchReaderBuilder::try_new(file)
            .map_err(|e| PiCloudError::TelemetryQueryFailed {
                reason: format!("Failed to read Parquet {}: {e}", path.display()),
            })?
            .build()
            .map_err(|e| PiCloudError::TelemetryQueryFailed {
                reason: format!("Failed to build reader for {}: {e}", path.display()),
            })?;

    let mut spans = Vec::new();
    for batch_result in reader {
        let batch = batch_result.map_err(|e| PiCloudError::TelemetryQueryFailed {
            reason: format!("Failed to read batch from {}: {e}", path.display()),
        })?;

        let trace_ids = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "trace_id column is not Utf8".to_string(),
            })?;
        let span_ids = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "span_id column is not Utf8".to_string(),
            })?;
        let parent_ids = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "parent_span_id column is not Utf8".to_string(),
            })?;
        let operations = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "operation_name column is not Utf8".to_string(),
            })?;
        let services = batch
            .column(4)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "service_name column is not Utf8".to_string(),
            })?;
        let start_times = batch
            .column(5)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "start_time column is not Utf8".to_string(),
            })?;
        let end_times = batch
            .column(6)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "end_time column is not Utf8".to_string(),
            })?;
        let durations = batch
            .column(7)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "duration_ms column is not UInt64".to_string(),
            })?;
        let statuses = batch
            .column(8)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "status column is not Utf8".to_string(),
            })?;
        let attrs = batch
            .column(9)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "attributes column is not Utf8".to_string(),
            })?;

        for i in 0..batch.num_rows() {
            let start_time = DateTime::parse_from_rfc3339(start_times.value(i))
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let end_time = DateTime::parse_from_rfc3339(end_times.value(i))
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let attributes: serde_json::Value =
                serde_json::from_str(attrs.value(i)).unwrap_or(serde_json::json!({}));

            spans.push(SpanRecord {
                trace_id: trace_ids.value(i).to_string(),
                span_id: span_ids.value(i).to_string(),
                parent_span_id: if parent_ids.is_null(i) {
                    None
                } else {
                    Some(parent_ids.value(i).to_string())
                },
                operation_name: operations.value(i).to_string(),
                service_name: services.value(i).to_string(),
                start_time,
                end_time,
                duration_ms: durations.value(i),
                status: statuses.value(i).to_string(),
                attributes,
            });
        }
    }

    Ok(spans)
}

/// Read all MetricRecords from a single Parquet file.
fn read_metrics_from_parquet(path: &Path) -> Result<Vec<MetricRecord>> {
    let file = std::fs::File::open(path).map_err(|e| PiCloudError::TelemetryQueryFailed {
        reason: format!("Failed to open {}: {e}", path.display()),
    })?;

    let reader =
        ParquetRecordBatchReaderBuilder::try_new(file)
            .map_err(|e| PiCloudError::TelemetryQueryFailed {
                reason: format!("Failed to read Parquet {}: {e}", path.display()),
            })?
            .build()
            .map_err(|e| PiCloudError::TelemetryQueryFailed {
                reason: format!("Failed to build reader for {}: {e}", path.display()),
            })?;

    let mut metrics = Vec::new();
    for batch_result in reader {
        let batch = batch_result.map_err(|e| PiCloudError::TelemetryQueryFailed {
            reason: format!("Failed to read batch from {}: {e}", path.display()),
        })?;

        let names = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "name column is not Utf8".to_string(),
            })?;
        let values = batch
            .column(1)
            .as_any()
            .downcast_ref::<Float64Array>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "value column is not Float64".to_string(),
            })?;
        let units = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "unit column is not Utf8".to_string(),
            })?;
        let types = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "metric_type column is not Utf8".to_string(),
            })?;
        let services = batch
            .column(4)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "service_name column is not Utf8".to_string(),
            })?;
        let timestamps = batch
            .column(5)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "timestamp column is not Utf8".to_string(),
            })?;
        let attrs = batch
            .column(6)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| PiCloudError::TelemetryQueryFailed {
                reason: "attributes column is not Utf8".to_string(),
            })?;

        for i in 0..batch.num_rows() {
            let timestamp = DateTime::parse_from_rfc3339(timestamps.value(i))
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let attributes: serde_json::Value =
                serde_json::from_str(attrs.value(i)).unwrap_or(serde_json::json!({}));

            metrics.push(MetricRecord {
                name: names.value(i).to_string(),
                value: values.value(i),
                unit: units.value(i).to_string(),
                metric_type: types.value(i).to_string(),
                service_name: services.value(i).to_string(),
                timestamp,
                attributes,
            });
        }
    }

    Ok(metrics)
}

// ---------------------------------------------------------------------------
// ParquetTelemetryStore
// ---------------------------------------------------------------------------

/// Parquet-backed telemetry store with hourly directory partitioning (ADR-046).
///
/// Data is written as Apache Parquet files into hourly partition directories:
///   `{base_path}/traces/{YYYY-MM-DDTHH}/part-NNNN.parquet`
///   `{base_path}/metrics/{YYYY-MM-DDTHH}/part-NNNN.parquet`
///
/// Each `write_spans` / `write_metrics` call creates a new part file in the
/// appropriate hourly partition. Reads scan all part files in the matching
/// hourly partitions.
pub struct ParquetTelemetryStore {
    base_path: PathBuf,
    /// Monotonic part counter per partition (simple, avoids collisions).
    part_counter: AtomicU32,
    /// Mutex to serialize writes within the same process.
    write_lock: Arc<Mutex<()>>,
    /// Retention period in hours (default: 168 = 7 days).
    retention_hours: u64,
}

impl ParquetTelemetryStore {
    /// Create a new Parquet telemetry store at the given base path.
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        let base_path = base_path.into();
        let _ = std::fs::create_dir_all(base_path.join("traces"));
        let _ = std::fs::create_dir_all(base_path.join("metrics"));

        Self {
            base_path,
            part_counter: AtomicU32::new(1),
            write_lock: Arc::new(Mutex::new(())),
            retention_hours: 168,
        }
    }

    /// Set the retention period in hours.
    pub fn with_retention_hours(mut self, hours: u64) -> Self {
        self.retention_hours = hours;
        self
    }

    /// Return the base path of this store.
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    /// Compute the hourly partition directory for a signal and timestamp.
    ///
    /// Format: `{base}/{signal}/{YYYY-MM-DDTHH}/` (per ADR-046).
    fn partition_dir(&self, signal: &str, ts: &DateTime<Utc>) -> PathBuf {
        let partition = ts.format("%Y-%m-%dT%H").to_string();
        self.base_path.join(signal).join(partition)
    }

    /// Allocate the next part file path inside a partition directory.
    fn next_part_path(&self, partition_dir: &Path) -> PathBuf {
        let n = self.part_counter.fetch_add(1, Ordering::Relaxed);
        partition_dir.join(format!("part-{n:04}.parquet"))
    }

    /// Write a RecordBatch as a Parquet file into the given partition directory.
    fn write_parquet(&self, partition_dir: &Path, batch: &RecordBatch) -> Result<PathBuf> {
        std::fs::create_dir_all(partition_dir).map_err(|e| PiCloudError::TelemetryWriteFailed {
            reason: format!("Failed to create partition dir {}: {e}", partition_dir.display()),
        })?;

        let path = self.next_part_path(partition_dir);
        let file =
            std::fs::File::create(&path).map_err(|e| PiCloudError::TelemetryWriteFailed {
                reason: format!("Failed to create {}: {e}", path.display()),
            })?;

        let props = WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .build();

        let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(props)).map_err(|e| {
            PiCloudError::TelemetryWriteFailed {
                reason: format!("Failed to create Parquet writer: {e}"),
            }
        })?;

        writer
            .write(batch)
            .map_err(|e| PiCloudError::TelemetryWriteFailed {
                reason: format!("Failed to write Parquet batch: {e}"),
            })?;

        writer
            .close()
            .map_err(|e| PiCloudError::TelemetryWriteFailed {
                reason: format!("Failed to close Parquet writer: {e}"),
            })?;

        debug!(path = %path.display(), rows = batch.num_rows(), "Wrote Parquet file");
        Ok(path)
    }

    /// Collect all Parquet file paths inside a signal directory that fall
    /// within the `[from, to]` time range based on the hourly partition name.
    fn partition_files_in_range(
        &self,
        signal: &str,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Vec<PathBuf> {
        let signal_dir = self.base_path.join(signal);
        if !signal_dir.exists() {
            return Vec::new();
        }

        let from_partition = from.format("%Y-%m-%dT%H").to_string();
        let to_partition = to.format("%Y-%m-%dT%H").to_string();

        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&signal_dir) {
            for entry in entries.flatten() {
                let dir_name = entry.file_name();
                let name = dir_name.to_string_lossy();

                // Include partition directories that fall within [from_partition, to_partition]
                if name.as_ref() >= from_partition.as_str()
                    && name.as_ref() <= to_partition.as_str()
                {
                    let part_dir = entry.path();
                    if part_dir.is_dir() {
                        if let Ok(parts) = std::fs::read_dir(&part_dir) {
                            for part in parts.flatten() {
                                let part_path = part.path();
                                if part_path
                                    .extension()
                                    .map_or(false, |ext| ext == "parquet")
                                {
                                    files.push(part_path);
                                }
                            }
                        }
                    }
                }
            }
        }

        files.sort();
        files
    }

    /// Start the retention cleanup loop as a background task.
    /// Runs every hour and deletes partition directories older than the retention window.
    pub fn start_retention_cleanup(&self) -> tokio::task::JoinHandle<()> {
        let base_path = self.base_path.clone();
        let retention_hours = self.retention_hours;

        tokio::spawn(async move {
            let interval = tokio::time::Duration::from_secs(3600);
            info!(
                retention_hours = retention_hours,
                "Parquet telemetry retention cleanup started"
            );

            loop {
                tokio::time::sleep(interval).await;

                let cutoff = Utc::now() - chrono::Duration::hours(retention_hours as i64);
                let cutoff_str = cutoff.format("%Y-%m-%dT%H").to_string();

                for signal in &["traces", "metrics"] {
                    let dir = base_path.join(signal);
                    if !dir.exists() {
                        continue;
                    }

                    match std::fs::read_dir(&dir) {
                        Ok(entries) => {
                            for entry in entries.flatten() {
                                let dirname = entry.file_name();
                                let name = dirname.to_string_lossy();
                                if name.as_ref() < cutoff_str.as_str() {
                                    if let Err(e) = std::fs::remove_dir_all(entry.path()) {
                                        warn!(
                                            dir = %entry.path().display(),
                                            error = %e,
                                            "Failed to remove expired partition"
                                        );
                                    } else {
                                        debug!(
                                            dir = %entry.path().display(),
                                            "Removed expired Parquet partition"
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

#[async_trait]
impl TelemetryStore for ParquetTelemetryStore {
    async fn write_spans(&self, spans: Vec<SpanRecord>) -> Result<()> {
        if spans.is_empty() {
            return Ok(());
        }

        let _lock = self.write_lock.lock().await;

        // Group spans by hourly partition based on start_time
        let mut by_partition: std::collections::HashMap<String, Vec<&SpanRecord>> =
            std::collections::HashMap::new();
        for span in &spans {
            let key = span.start_time.format("%Y-%m-%dT%H").to_string();
            by_partition.entry(key).or_default().push(span);
        }

        for (partition_key, partition_spans) in &by_partition {
            // Build a RecordBatch for this partition's spans
            let owned: Vec<SpanRecord> = partition_spans.iter().map(|s| (*s).clone()).collect();
            let batch = spans_to_record_batch(&owned)?;

            let partition_dir = self.base_path.join("traces").join(partition_key);
            self.write_parquet(&partition_dir, &batch)?;
        }

        debug!(count = spans.len(), "Wrote spans to Parquet store");
        Ok(())
    }

    async fn write_metrics(&self, metrics: Vec<MetricRecord>) -> Result<()> {
        if metrics.is_empty() {
            return Ok(());
        }

        let _lock = self.write_lock.lock().await;

        // Group metrics by hourly partition based on timestamp
        let mut by_partition: std::collections::HashMap<String, Vec<&MetricRecord>> =
            std::collections::HashMap::new();
        for metric in &metrics {
            let key = metric.timestamp.format("%Y-%m-%dT%H").to_string();
            by_partition.entry(key).or_default().push(metric);
        }

        for (partition_key, partition_metrics) in &by_partition {
            let owned: Vec<MetricRecord> = partition_metrics.iter().map(|m| (*m).clone()).collect();
            let batch = metrics_to_record_batch(&owned)?;

            let partition_dir = self.base_path.join("metrics").join(partition_key);
            self.write_parquet(&partition_dir, &batch)?;
        }

        debug!(count = metrics.len(), "Wrote metrics to Parquet store");
        Ok(())
    }

    async fn query_spans(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        filter: TelemetryFilter,
    ) -> Result<Vec<SpanRecord>> {
        let files = self.partition_files_in_range("traces", from, to);

        let mut all_spans = Vec::new();
        for file in &files {
            match read_spans_from_parquet(file) {
                Ok(spans) => all_spans.extend(spans),
                Err(e) => {
                    warn!(file = %file.display(), error = %e, "Skipping unreadable Parquet file");
                }
            }
        }

        // Apply filters
        let filtered = all_spans
            .into_iter()
            .filter(|s| {
                if s.start_time < from || s.start_time > to {
                    return false;
                }
                if let Some(ref svc) = filter.service_name {
                    if &s.service_name != svc {
                        return false;
                    }
                }
                if let Some(ref op) = filter.operation_name {
                    if &s.operation_name != op {
                        return false;
                    }
                }
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
        let files = self.partition_files_in_range("metrics", from, to);

        let mut all_metrics = Vec::new();
        for file in &files {
            match read_metrics_from_parquet(file) {
                Ok(metrics) => all_metrics.extend(metrics),
                Err(e) => {
                    warn!(file = %file.display(), error = %e, "Skipping unreadable Parquet file");
                }
            }
        }

        let filtered = all_metrics
            .into_iter()
            .filter(|m| {
                if m.timestamp < from || m.timestamp > to {
                    return false;
                }
                if let Some(ref svc) = filter.service_name {
                    if &m.service_name != svc {
                        return false;
                    }
                }
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

    fn temp_store() -> (ParquetTelemetryStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "picloud-parquet-test-{}",
            uuid::Uuid::new_v4()
        ));
        let store = ParquetTelemetryStore::new(&dir);
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

        // Verify the partition directory and Parquet file exist
        let partition = now.format("%Y-%m-%dT%H").to_string();
        let partition_dir = dir.join("traces").join(&partition);
        assert!(partition_dir.exists(), "Hourly partition directory should exist");

        let parquet_files: Vec<_> = std::fs::read_dir(&partition_dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.path()
                    .extension()
                    .map_or(false, |ext| ext == "parquet")
            })
            .collect();
        assert!(
            !parquet_files.is_empty(),
            "Should have at least one .parquet file"
        );

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

        let _ = std::fs::remove_dir_all(&dir);
    }
}
