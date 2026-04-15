//! FT-045 — DataFusion SQL over Parquet via picloud telemetry query
//!
//! Covers TC-258, TC-315.
//! These tests verify that:
//! 1. SQL queries executed via DataFusion return correct traces from Parquet files
//! 2. WHERE filtering, aggregation (GROUP BY/COUNT), and ORDER BY work
//! 3. The query_sql trait method returns well-formed (columns, rows) tuples
//! 4. Empty result sets are handled gracefully
//! 5. Both traces and metrics tables can be queried via SQL

use chrono::{Duration, TimeZone, Utc};

use picloud_domain::events::{MetricRecord, SpanRecord};
use picloud_domain::traits::TelemetryStore;
use picloud_http::ParquetTelemetryStore;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_store() -> (ParquetTelemetryStore, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "picloud-datafusion-ft045-{}",
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

// ===========================================================================
// TC-258 — DataFusion SQL query returns traces from Parquet store (scenario)
// ===========================================================================

/// TC-258: Scenario test — DataFusion executes SQL queries over Parquet-stored
/// telemetry data and returns correct, filtered results.
///
/// Steps:
/// 1. Write a batch of spans to the Parquet store across multiple hours
/// 2. Execute `SELECT * FROM traces` — verify all rows returned
/// 3. Execute `SELECT * FROM traces WHERE service_name = 'api-server'` — filter
/// 4. Execute `SELECT * FROM traces WHERE duration_ms > 100` — numeric filter
/// 5. Execute `SELECT service_name, COUNT(*) as cnt FROM traces GROUP BY service_name` — aggregation
/// 6. Execute `SELECT * FROM traces ORDER BY duration_ms DESC LIMIT 2` — ordering + limit
/// 7. Query an empty store — verify empty result set
/// 8. Write metrics and query via `SELECT * FROM metrics WHERE service_name = 'api-server'`
/// 9. Verify column names match the SQL projection
#[tokio::test]
async fn tc258_datafusion_sql_query_returns_traces_from_parquet_store() {
    let (store, dir) = temp_store();

    let h10 = Utc.with_ymd_and_hms(2026, 4, 15, 10, 30, 0).unwrap();
    let h11 = Utc.with_ymd_and_hms(2026, 4, 15, 11, 15, 0).unwrap();

    // ----- Step 1: Write spans across two hours -----
    let spans = vec![
        make_span("t1", "s1", "GET /api/products", "api-server", h10, 50),
        make_span("t1", "s2", "db.query", "api-server", h10, 20),
        make_span("t2", "s3", "POST /orders", "order-svc", h10, 150),
        make_span("t3", "s4", "GET /health", "api-server", h11, 5),
        make_span("t4", "s5", "PUT /items", "order-svc", h11, 300),
    ];
    store.write_spans(spans).await.unwrap();

    // ----- Step 2: SELECT * FROM traces — all rows -----
    let (columns, rows) = store
        .query_sql("SELECT * FROM traces", "traces")
        .await
        .unwrap();

    assert!(!columns.is_empty(), "Should return column names");
    assert!(
        columns.contains(&"trace_id".to_string()),
        "Columns should include trace_id"
    );
    assert!(
        columns.contains(&"service_name".to_string()),
        "Columns should include service_name"
    );
    assert_eq!(rows.len(), 5, "SELECT * should return all 5 spans");

    // ----- Step 3: WHERE service_name filter -----
    let (_, rows) = store
        .query_sql(
            "SELECT * FROM traces WHERE service_name = 'api-server'",
            "traces",
        )
        .await
        .unwrap();

    assert_eq!(
        rows.len(),
        3,
        "api-server has 3 spans (s1, s2, s4)"
    );
    for row in &rows {
        assert_eq!(
            row.get("service_name").and_then(|v| v.as_str()),
            Some("api-server"),
            "All filtered rows should be api-server"
        );
    }

    // ----- Step 4: WHERE duration_ms > 100 -----
    let (_, rows) = store
        .query_sql(
            "SELECT * FROM traces WHERE duration_ms > 100",
            "traces",
        )
        .await
        .unwrap();

    assert_eq!(
        rows.len(),
        2,
        "Two spans have duration_ms > 100 (150 and 300)"
    );
    for row in &rows {
        let dur = row
            .get("duration_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        assert!(dur > 100, "Filtered rows should have duration_ms > 100");
    }

    // ----- Step 5: GROUP BY + COUNT aggregation -----
    let (columns, rows) = store
        .query_sql(
            "SELECT service_name, COUNT(*) as cnt FROM traces GROUP BY service_name ORDER BY service_name",
            "traces",
        )
        .await
        .unwrap();

    assert!(
        columns.contains(&"service_name".to_string()),
        "Aggregation result should have service_name column"
    );
    assert!(
        columns.contains(&"cnt".to_string()),
        "Aggregation result should have cnt column"
    );
    assert_eq!(rows.len(), 2, "Two distinct services");

    let api_row = rows
        .iter()
        .find(|r| r.get("service_name").and_then(|v| v.as_str()) == Some("api-server"))
        .expect("Should find api-server in GROUP BY results");
    let api_count = api_row.get("cnt").and_then(|v| v.as_i64()).unwrap_or(0);
    assert_eq!(api_count, 3, "api-server has 3 spans");

    let order_row = rows
        .iter()
        .find(|r| r.get("service_name").and_then(|v| v.as_str()) == Some("order-svc"))
        .expect("Should find order-svc in GROUP BY results");
    let order_count = order_row.get("cnt").and_then(|v| v.as_i64()).unwrap_or(0);
    assert_eq!(order_count, 2, "order-svc has 2 spans");

    // ----- Step 6: ORDER BY + LIMIT -----
    let (_, rows) = store
        .query_sql(
            "SELECT operation_name, duration_ms FROM traces ORDER BY duration_ms DESC LIMIT 2",
            "traces",
        )
        .await
        .unwrap();

    assert_eq!(rows.len(), 2, "LIMIT 2 should return 2 rows");
    let first_dur = rows[0]
        .get("duration_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let second_dur = rows[1]
        .get("duration_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        first_dur >= second_dur,
        "Results should be ordered by duration_ms DESC"
    );
    assert_eq!(first_dur, 300, "Highest duration should be 300ms");
    assert_eq!(second_dur, 150, "Second highest should be 150ms");

    // ----- Step 7: Empty store returns empty results -----
    let (empty_store, empty_dir) = temp_store();
    let (_, empty_rows) = empty_store
        .query_sql("SELECT * FROM traces", "traces")
        .await
        .unwrap();
    assert!(
        empty_rows.is_empty(),
        "Empty store should return no rows"
    );
    let _ = std::fs::remove_dir_all(&empty_dir);

    // ----- Step 8: Metrics SQL query -----
    let metrics = vec![
        make_metric("http_latency_ms", 42.5, "api-server", h10),
        make_metric("cpu_usage_pct", 65.3, "api-server", h10),
        make_metric("error_rate", 0.01, "order-svc", h10),
    ];
    store.write_metrics(metrics).await.unwrap();

    let (_, rows) = store
        .query_sql(
            "SELECT * FROM metrics WHERE service_name = 'api-server'",
            "metrics",
        )
        .await
        .unwrap();

    assert_eq!(rows.len(), 2, "api-server has 2 metrics");
    for row in &rows {
        assert_eq!(
            row.get("service_name").and_then(|v| v.as_str()),
            Some("api-server")
        );
    }

    // ----- Step 9: Verify column names match projection -----
    let (columns, _) = store
        .query_sql(
            "SELECT trace_id, operation_name FROM traces",
            "traces",
        )
        .await
        .unwrap();

    assert_eq!(columns.len(), 2, "Projection should return exactly 2 columns");
    assert_eq!(columns[0], "trace_id");
    assert_eq!(columns[1], "operation_name");

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

// ===========================================================================
// TC-315 — DataFusion exit — SQL query over Parquet returns traces
// ===========================================================================

/// TC-315: Exit-criteria test — end-to-end verification that DataFusion SQL
/// queries over Parquet-stored telemetry data produce correct, complete results.
///
/// This validates:
/// 1. A realistic workload of spans written to Parquet can be queried via SQL
/// 2. SQL filtering returns the correct subset of traces
/// 3. Column and row structure is well-formed JSON
/// 4. The query result total matches the actual row count
/// 5. Data survives a store recreation (persistence check)
/// 6. Both signal types (traces, metrics) work with SQL
/// 7. Complex SQL with multiple conditions works
#[tokio::test]
async fn tc315_datafusion_exit_sql_query_over_parquet_returns_traces() {
    let dir = std::env::temp_dir().join(format!(
        "picloud-datafusion-tc315-{}",
        uuid::Uuid::new_v4()
    ));

    let h10 = Utc.with_ymd_and_hms(2026, 4, 15, 10, 0, 0).unwrap();
    let h11 = Utc.with_ymd_and_hms(2026, 4, 15, 11, 0, 0).unwrap();

    // ----- Phase 1: Write data with the first store instance -----
    {
        let store = ParquetTelemetryStore::new(&dir);

        // Write a realistic batch of traces
        let mut spans = Vec::new();
        for i in 0..50 {
            let service = if i % 3 == 0 {
                "api-server"
            } else if i % 3 == 1 {
                "order-svc"
            } else {
                "payment-svc"
            };
            let operation = match i % 4 {
                0 => "GET /products",
                1 => "POST /orders",
                2 => "db.query",
                _ => "cache.get",
            };
            let hour = if i < 25 { h10 } else { h11 };
            let dur = (i as u64 + 1) * 10; // 10, 20, 30, ... 500

            spans.push(make_span(
                &format!("trace-{i:03}"),
                &format!("span-{i:03}"),
                operation,
                service,
                hour + Duration::minutes(i as i64 % 55),
                dur,
            ));
        }
        store.write_spans(spans).await.unwrap();

        // Write metrics
        let metrics = vec![
            make_metric("request_count", 1000.0, "api-server", h10),
            make_metric("p99_latency_ms", 450.0, "api-server", h10),
            make_metric("error_rate", 0.005, "order-svc", h11),
        ];
        store.write_metrics(metrics).await.unwrap();
    }
    // Store dropped — data persisted on disk.

    // ----- Phase 2: Recreate store and verify SQL queries -----
    let store = ParquetTelemetryStore::new(&dir);

    // 2a. SELECT * should return all 50 spans
    let (columns, rows) = store
        .query_sql("SELECT * FROM traces", "traces")
        .await
        .unwrap();

    assert!(!columns.is_empty(), "Column list should not be empty");
    assert_eq!(
        rows.len(),
        50,
        "All 50 spans should be queryable after store recreation"
    );

    // 2b. Verify row structure — each row should be a JSON object
    for row in &rows {
        assert!(row.is_object(), "Each row should be a JSON object");
        assert!(
            row.get("trace_id").is_some(),
            "Each row should have trace_id"
        );
        assert!(
            row.get("service_name").is_some(),
            "Each row should have service_name"
        );
        assert!(
            row.get("duration_ms").is_some(),
            "Each row should have duration_ms"
        );
    }

    // 2c. Filtered query — api-server spans
    let (_, api_rows) = store
        .query_sql(
            "SELECT * FROM traces WHERE service_name = 'api-server'",
            "traces",
        )
        .await
        .unwrap();

    // api-server: indices where i % 3 == 0 → 0,3,6,...,48 → 17 spans
    assert_eq!(
        api_rows.len(),
        17,
        "api-server should have 17 spans (every 3rd of 50)"
    );
    for row in &api_rows {
        assert_eq!(
            row.get("service_name").and_then(|v| v.as_str()),
            Some("api-server")
        );
    }

    // 2d. Complex multi-condition WHERE
    let (_, complex_rows) = store
        .query_sql(
            "SELECT * FROM traces WHERE service_name = 'order-svc' AND duration_ms >= 200",
            "traces",
        )
        .await
        .unwrap();

    // order-svc: indices where i % 3 == 1 → 1,4,7,...,49
    // duration = (i+1)*10, so duration >= 200 means i+1 >= 20, i.e. i >= 19
    // order-svc indices >= 19: 19,22,25,28,31,34,37,40,43,46,49
    // That's 11 spans
    for row in &complex_rows {
        let svc = row.get("service_name").and_then(|v| v.as_str()).unwrap();
        let dur = row.get("duration_ms").and_then(|v| v.as_u64()).unwrap();
        assert_eq!(svc, "order-svc", "Should only be order-svc");
        assert!(dur >= 200, "Should have duration >= 200, got {dur}");
    }
    assert!(
        !complex_rows.is_empty(),
        "Complex query should return some results"
    );

    // 2e. Metrics SQL query
    let (metric_cols, metric_rows) = store
        .query_sql("SELECT * FROM metrics", "metrics")
        .await
        .unwrap();

    assert!(!metric_cols.is_empty(), "Metric columns should not be empty");
    assert_eq!(
        metric_rows.len(),
        3,
        "All 3 metrics should be returned"
    );

    // 2f. Metrics filter
    let (_, api_metrics) = store
        .query_sql(
            "SELECT name, value FROM metrics WHERE service_name = 'api-server' ORDER BY value DESC",
            "metrics",
        )
        .await
        .unwrap();

    assert_eq!(api_metrics.len(), 2, "api-server has 2 metrics");
    // Ordered by value DESC: request_count (1000.0) first, p99_latency_ms (450.0) second
    let first_val = api_metrics[0]
        .get("value")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let second_val = api_metrics[1]
        .get("value")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    assert!(
        first_val >= second_val,
        "Should be ordered by value DESC"
    );

    // 2g. Aggregation with HAVING (advanced SQL)
    let (_, agg_rows) = store
        .query_sql(
            "SELECT service_name, COUNT(*) as span_count, AVG(duration_ms) as avg_dur \
             FROM traces \
             GROUP BY service_name \
             HAVING COUNT(*) > 10 \
             ORDER BY span_count DESC",
            "traces",
        )
        .await
        .unwrap();

    // api-server: 17, order-svc: 17, payment-svc: 16
    // All > 10, so all three should appear
    assert_eq!(
        agg_rows.len(),
        3,
        "All three services have > 10 spans"
    );

    // Verify the first row (highest count) has a valid span_count
    let top_count = agg_rows[0]
        .get("span_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    assert!(
        top_count >= 16,
        "Top service should have at least 16 spans, got {top_count}"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}
