//! FT-044 — Parquet time-series store — traces, metrics, logs with hourly partitioning
//!
//! Covers TC-257, TC-314.
//! These tests verify that:
//! 1. The ParquetTelemetryStore writes spans and metrics as Parquet files
//! 2. Files are organized into hourly partition directories (YYYY-MM-DDTHH/)
//! 3. Written data can be queried back with correct values
//! 4. Parquet files have valid format (magic bytes PAR1)
//! 5. Filtering by service_name, operation_name, and min_duration_ms works
//! 6. Multiple partitions are created for data spanning multiple hours

use chrono::{Duration, TimeZone, Utc};

use picloud_domain::events::{MetricRecord, SpanRecord, TelemetryFilter};
use picloud_domain::traits::TelemetryStore;
use picloud_http::ParquetTelemetryStore;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_store() -> (ParquetTelemetryStore, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "picloud-parquet-ft044-{}",
        uuid::Uuid::new_v4()
    ));
    let store = ParquetTelemetryStore::new(&dir);
    (store, dir)
}

fn make_span(
    trace_id: &str,
    span_id: &str,
    operation: &str,
    service: &str,
    start: chrono::DateTime<chrono::Utc>,
    duration_ms: u64,
) -> SpanRecord {
    SpanRecord {
        trace_id: trace_id.to_string(),
        span_id: span_id.to_string(),
        parent_span_id: None,
        operation_name: operation.to_string(),
        service_name: service.to_string(),
        start_time: start,
        end_time: start + Duration::milliseconds(duration_ms as i64),
        duration_ms,
        status: "OK".to_string(),
        attributes: serde_json::json!({"test": true}),
    }
}

fn make_metric(
    name: &str,
    value: f64,
    service: &str,
    ts: chrono::DateTime<chrono::Utc>,
) -> MetricRecord {
    MetricRecord {
        name: name.to_string(),
        value,
        unit: "ms".to_string(),
        metric_type: "gauge".to_string(),
        service_name: service.to_string(),
        timestamp: ts,
        attributes: serde_json::json!({}),
    }
}

/// Read 4 bytes from a file to check for Parquet magic bytes.
fn has_parquet_magic(path: &std::path::Path) -> bool {
    use std::io::Read;
    let mut file = std::fs::File::open(path).unwrap();
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).unwrap();
    &magic == b"PAR1"
}

// ===========================================================================
// TC-257 — Parquet time-series store writes hourly partitioned trace files
// ===========================================================================

/// TC-257: Scenario test — the ParquetTelemetryStore writes trace spans as
/// Parquet files in hourly partition directories and reads them back correctly.
///
/// Steps:
/// 1. Create a ParquetTelemetryStore with a temp directory
/// 2. Write a batch of spans with a known timestamp
/// 3. Verify the partition directory `traces/{YYYY-MM-DDTHH}/` was created
/// 4. Verify at least one .parquet file exists in the partition directory
/// 5. Verify the .parquet file starts with the PAR1 magic bytes
/// 6. Query spans back and verify all fields match the written data
/// 7. Write spans across TWO different hours — verify separate partition dirs
/// 8. Verify filtering by service_name works on queried results
/// 9. Verify filtering by operation_name works on queried results
/// 10. Verify filtering by min_duration_ms works on queried results
/// 11. Write metrics and verify they also produce hourly partitioned Parquet files
/// 12. Query metrics back and verify values are preserved
#[tokio::test]
async fn tc257_parquet_time_series_store_writes_hourly_partitioned_trace_files() {
    let (store, dir) = temp_store();

    // Use a fixed, well-known timestamp for deterministic partition names
    let hour1 = Utc.with_ymd_and_hms(2026, 4, 15, 10, 30, 0).unwrap();
    let hour2 = Utc.with_ymd_and_hms(2026, 4, 15, 11, 15, 0).unwrap();

    // ----- Step 2: Write a batch of spans in hour1 -----
    let spans_h1 = vec![
        make_span("trace-001", "span-001", "GET /api/products", "api-server", hour1, 50),
        make_span("trace-001", "span-002", "db.query", "api-server", hour1, 20),
        make_span("trace-002", "span-003", "POST /orders", "order-svc", hour1, 150),
    ];
    store.write_spans(spans_h1).await.unwrap();

    // ----- Step 3: Verify the hourly partition directory exists -----
    let partition_h1 = dir.join("traces").join("2026-04-15T10");
    assert!(
        partition_h1.exists(),
        "Partition directory traces/2026-04-15T10/ should exist"
    );
    assert!(
        partition_h1.is_dir(),
        "Partition path should be a directory"
    );

    // ----- Step 4: Verify at least one .parquet file exists -----
    let parquet_files_h1: Vec<_> = std::fs::read_dir(&partition_h1)
        .unwrap()
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .map_or(false, |ext| ext == "parquet")
        })
        .collect();
    assert!(
        !parquet_files_h1.is_empty(),
        "Partition should contain at least one .parquet file"
    );

    // ----- Step 5: Verify Parquet magic bytes (PAR1) -----
    for file_entry in &parquet_files_h1 {
        assert!(
            has_parquet_magic(&file_entry.path()),
            "Parquet file {} should start with PAR1 magic bytes",
            file_entry.path().display()
        );
    }

    // ----- Step 6: Query spans back and verify all fields -----
    let queried = store
        .query_spans(
            hour1 - Duration::minutes(5),
            hour1 + Duration::minutes(35),
            TelemetryFilter::default(),
        )
        .await
        .unwrap();
    assert_eq!(queried.len(), 3, "Should read back all 3 spans from hour1");

    // Verify specific span fields
    let product_span = queried
        .iter()
        .find(|s| s.span_id == "span-001")
        .expect("Should find span-001");
    assert_eq!(product_span.trace_id, "trace-001");
    assert_eq!(product_span.operation_name, "GET /api/products");
    assert_eq!(product_span.service_name, "api-server");
    assert_eq!(product_span.duration_ms, 50);
    assert_eq!(product_span.status, "OK");
    assert!(product_span.parent_span_id.is_none());

    // ----- Step 7: Write spans in a different hour — verify separate partition -----
    let spans_h2 = vec![
        make_span("trace-003", "span-004", "GET /health", "api-server", hour2, 5),
    ];
    store.write_spans(spans_h2).await.unwrap();

    let partition_h2 = dir.join("traces").join("2026-04-15T11");
    assert!(
        partition_h2.exists(),
        "Second partition directory traces/2026-04-15T11/ should exist"
    );

    // Both partitions should now exist independently
    assert!(partition_h1.exists(), "First partition should still exist");
    assert!(partition_h2.exists(), "Second partition should exist");

    // Query across both hours
    let all_spans = store
        .query_spans(
            hour1 - Duration::minutes(5),
            hour2 + Duration::minutes(30),
            TelemetryFilter::default(),
        )
        .await
        .unwrap();
    assert_eq!(all_spans.len(), 4, "Should have 3 spans from hour1 + 1 from hour2");

    // ----- Step 8: Filter by service_name -----
    let api_only = store
        .query_spans(
            hour1 - Duration::minutes(5),
            hour2 + Duration::minutes(30),
            TelemetryFilter {
                service_name: Some("api-server".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(api_only.len(), 3, "api-server has 3 spans across both hours");
    for span in &api_only {
        assert_eq!(span.service_name, "api-server");
    }

    // ----- Step 9: Filter by operation_name -----
    let post_only = store
        .query_spans(
            hour1 - Duration::minutes(5),
            hour2 + Duration::minutes(30),
            TelemetryFilter {
                operation_name: Some("POST /orders".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(post_only.len(), 1, "Only one POST /orders span");
    assert_eq!(post_only[0].trace_id, "trace-002");

    // ----- Step 10: Filter by min_duration_ms -----
    let slow_only = store
        .query_spans(
            hour1 - Duration::minutes(5),
            hour2 + Duration::minutes(30),
            TelemetryFilter {
                min_duration_ms: Some(100),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(slow_only.len(), 1, "Only one span >= 100ms");
    assert_eq!(slow_only[0].span_id, "span-003");
    assert_eq!(slow_only[0].duration_ms, 150);

    // ----- Step 11: Write metrics and verify hourly partitioned Parquet files -----
    let metrics = vec![
        make_metric("http_latency_ms", 42.5, "api-server", hour1),
        make_metric("cpu_usage_pct", 65.3, "api-server", hour1),
    ];
    store.write_metrics(metrics).await.unwrap();

    let metrics_partition = dir.join("metrics").join("2026-04-15T10");
    assert!(
        metrics_partition.exists(),
        "Metrics partition directory should exist"
    );

    let metric_files: Vec<_> = std::fs::read_dir(&metrics_partition)
        .unwrap()
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .map_or(false, |ext| ext == "parquet")
        })
        .collect();
    assert!(
        !metric_files.is_empty(),
        "Metrics partition should have at least one .parquet file"
    );

    // Verify Parquet magic on metric files too
    for file_entry in &metric_files {
        assert!(
            has_parquet_magic(&file_entry.path()),
            "Metric Parquet file should start with PAR1 magic bytes"
        );
    }

    // ----- Step 12: Query metrics back and verify values -----
    let queried_metrics = store
        .query_metrics(
            hour1 - Duration::minutes(5),
            hour1 + Duration::minutes(35),
            TelemetryFilter::default(),
        )
        .await
        .unwrap();
    assert_eq!(queried_metrics.len(), 2, "Should read back both metrics");

    let latency = queried_metrics
        .iter()
        .find(|m| m.name == "http_latency_ms")
        .expect("Should find http_latency_ms");
    assert_eq!(latency.value, 42.5);
    assert_eq!(latency.unit, "ms");
    assert_eq!(latency.service_name, "api-server");

    let cpu = queried_metrics
        .iter()
        .find(|m| m.name == "cpu_usage_pct")
        .expect("Should find cpu_usage_pct");
    assert_eq!(cpu.value, 65.3);

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

// ===========================================================================
// TC-314 — Parquet store exit — hourly partitioned trace files written
// ===========================================================================

/// TC-314: Exit-criteria test — end-to-end verification that the Parquet
/// telemetry store produces valid, queryable hourly partitioned Parquet files
/// for both traces and metrics, with data integrity across write-read cycles.
///
/// This validates:
/// 1. The complete write → persist → read cycle for traces
/// 2. The complete write → persist → read cycle for metrics
/// 3. Hourly partition directory naming matches ADR-046 format (YYYY-MM-DDTHH)
/// 4. Parquet files are self-describing (valid format, readable by standard tools)
/// 5. Data survives a "new store instance" pointing at the same directory
/// 6. Empty time ranges return empty results without errors
/// 7. Mixed-hour data is correctly partitioned and queryable per-hour
#[tokio::test]
async fn tc314_parquet_store_exit_hourly_partitioned_trace_files_written() {
    let dir = std::env::temp_dir().join(format!(
        "picloud-parquet-tc314-{}",
        uuid::Uuid::new_v4()
    ));

    // Use timestamps that span multiple hours to test partitioning
    let h10 = Utc.with_ymd_and_hms(2026, 4, 15, 10, 0, 0).unwrap();
    let h11 = Utc.with_ymd_and_hms(2026, 4, 15, 11, 0, 0).unwrap();
    let h12 = Utc.with_ymd_and_hms(2026, 4, 15, 12, 0, 0).unwrap();

    // ----- Phase 1: Write data with the first store instance -----
    {
        let store = ParquetTelemetryStore::new(&dir);

        // Write traces spanning 3 hours
        let spans = vec![
            make_span("t1", "s1", "GET /products", "api-svc", h10 + Duration::minutes(5), 100),
            make_span("t1", "s2", "db.select", "api-svc", h10 + Duration::minutes(5), 30),
            make_span("t2", "s3", "POST /orders", "order-svc", h11 + Duration::minutes(10), 200),
            make_span("t3", "s4", "GET /health", "api-svc", h12 + Duration::minutes(1), 5),
        ];
        store.write_spans(spans).await.unwrap();

        // Write metrics in two different hours
        let metrics = vec![
            make_metric("request_count", 42.0, "api-svc", h10 + Duration::minutes(15)),
            make_metric("error_rate", 0.02, "order-svc", h11 + Duration::minutes(20)),
        ];
        store.write_metrics(metrics).await.unwrap();
    }
    // First store instance is dropped — data must persist on disk.

    // ----- Phase 2: Verify partition directory structure on disk -----
    let traces_dir = dir.join("traces");
    assert!(traces_dir.exists(), "traces/ directory should exist");

    // All three hourly partitions should exist for traces
    for partition_name in &["2026-04-15T10", "2026-04-15T11", "2026-04-15T12"] {
        let part_dir = traces_dir.join(partition_name);
        assert!(
            part_dir.exists() && part_dir.is_dir(),
            "Trace partition {partition_name}/ should be a directory"
        );

        // Each partition should contain at least one Parquet file
        let files: Vec<_> = std::fs::read_dir(&part_dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.path()
                    .extension()
                    .map_or(false, |ext| ext == "parquet")
            })
            .collect();
        assert!(
            !files.is_empty(),
            "Partition {partition_name}/ should contain .parquet files"
        );

        // Verify every Parquet file has valid magic bytes
        for f in &files {
            assert!(
                has_parquet_magic(&f.path()),
                "File {} should be a valid Parquet file (PAR1 magic)",
                f.path().display()
            );
        }
    }

    // Verify metrics partition structure
    let metrics_dir = dir.join("metrics");
    assert!(metrics_dir.exists(), "metrics/ directory should exist");

    for partition_name in &["2026-04-15T10", "2026-04-15T11"] {
        let part_dir = metrics_dir.join(partition_name);
        assert!(
            part_dir.exists() && part_dir.is_dir(),
            "Metrics partition {partition_name}/ should be a directory"
        );
    }

    // ----- Phase 3: Create a NEW store instance pointing at the same dir -----
    //       This proves data survives across store lifetimes (persistence).
    let store2 = ParquetTelemetryStore::new(&dir);

    // ----- Phase 4: Query traces from the new instance -----

    // Query all traces across all 3 hours
    let all_spans = store2
        .query_spans(
            h10 - Duration::minutes(1),
            h12 + Duration::minutes(30),
            TelemetryFilter::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        all_spans.len(),
        4,
        "New store instance should read all 4 spans from Parquet files"
    );

    // Verify data integrity on a specific span
    let order_span = all_spans
        .iter()
        .find(|s| s.operation_name == "POST /orders")
        .expect("Should find POST /orders span");
    assert_eq!(order_span.trace_id, "t2");
    assert_eq!(order_span.span_id, "s3");
    assert_eq!(order_span.service_name, "order-svc");
    assert_eq!(order_span.duration_ms, 200);
    assert_eq!(order_span.status, "OK");

    // Query only hour 10 — should get 2 spans
    let h10_spans = store2
        .query_spans(
            h10,
            h10 + Duration::minutes(59),
            TelemetryFilter::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        h10_spans.len(),
        2,
        "Hour 10 should contain exactly 2 spans"
    );

    // Query only hour 11 — should get 1 span
    let h11_spans = store2
        .query_spans(
            h11,
            h11 + Duration::minutes(59),
            TelemetryFilter::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        h11_spans.len(),
        1,
        "Hour 11 should contain exactly 1 span"
    );
    assert_eq!(h11_spans[0].operation_name, "POST /orders");

    // ----- Phase 5: Query metrics from the new instance -----
    let all_metrics = store2
        .query_metrics(
            h10 - Duration::minutes(1),
            h12 + Duration::minutes(30),
            TelemetryFilter::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        all_metrics.len(),
        2,
        "New store instance should read all 2 metrics"
    );

    let req_count = all_metrics
        .iter()
        .find(|m| m.name == "request_count")
        .expect("Should find request_count metric");
    assert_eq!(req_count.value, 42.0);
    assert_eq!(req_count.service_name, "api-svc");

    let err_rate = all_metrics
        .iter()
        .find(|m| m.name == "error_rate")
        .expect("Should find error_rate metric");
    assert_eq!(err_rate.value, 0.02);
    assert_eq!(err_rate.service_name, "order-svc");

    // Filter metrics by service name
    let api_metrics = store2
        .query_metrics(
            h10 - Duration::minutes(1),
            h12 + Duration::minutes(30),
            TelemetryFilter {
                service_name: Some("api-svc".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(api_metrics.len(), 1);
    assert_eq!(api_metrics[0].name, "request_count");

    // ----- Phase 6: Empty time range returns empty results -----
    let empty = store2
        .query_spans(
            Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2020, 1, 1, 1, 0, 0).unwrap(),
            TelemetryFilter::default(),
        )
        .await
        .unwrap();
    assert!(
        empty.is_empty(),
        "Querying a time range with no data should return empty"
    );

    let empty_metrics = store2
        .query_metrics(
            Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2020, 1, 1, 1, 0, 0).unwrap(),
            TelemetryFilter::default(),
        )
        .await
        .unwrap();
    assert!(
        empty_metrics.is_empty(),
        "Querying metrics in empty range should return empty"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}
