//! FT-046 — Metric aggregator — OTel stream to MetricRecorded events every 15s
//!
//! Covers TC-259, TC-316.
//! These tests verify that:
//! 1. The OtelAggregator subscribes to the OtelStream, aggregates OTel metrics,
//!    and emits MetricRecorded events to the platform event log every interval.
//! 2. The emitted MetricRecorded events contain correct aggregated metric values.
//! 3. The emission schedule is regular (every aggregation interval).

use std::sync::Arc;

use chrono::Utc;

use picloud_domain::events::{MetricRecord, MetricRecordedPayload};
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_events::InMemoryEventLog;
use picloud_http::{JsonlTelemetryStore, OtelAggregator, OtelDatum, OtelStream};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_metric(name: &str, value: f64, unit: &str, service: &str) -> MetricRecord {
    MetricRecord {
        name: name.to_string(),
        value,
        unit: unit.to_string(),
        metric_type: "gauge".to_string(),
        service_name: service.to_string(),
        timestamp: Utc::now(),
        attributes: serde_json::json!({}),
    }
}

fn new_iri_builder() -> IriBuilder {
    IriBuilder::new(ClusterDomain::default())
}

fn test_infra() -> (
    Arc<OtelStream>,
    Arc<InMemoryEventLog>,
    ResourceIri,
) {
    let iri_builder = new_iri_builder();
    let node_iri = iri_builder.node("test-node-1");
    let event_log = Arc::new(InMemoryEventLog::new());
    let otel_stream = Arc::new(OtelStream::new(4096));
    (otel_stream, event_log, node_iri)
}

fn make_telemetry_store() -> Arc<JsonlTelemetryStore> {
    let dir = std::env::temp_dir().join(format!("picloud-ft046-{}", uuid::Uuid::new_v4()));
    Arc::new(JsonlTelemetryStore::new(&dir))
}

/// Wait for at least `min_count` events of type `event_type` to appear in the
/// event log, timing out after `timeout_ms` milliseconds.
async fn wait_for_events(
    event_log: &InMemoryEventLog,
    event_type: &str,
    min_count: usize,
    timeout_ms: u64,
) -> Vec<picloud_domain::events::EventEnvelope> {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);

    loop {
        let events = event_log.events_since(0).await;
        let matching: Vec<_> = events
            .into_iter()
            .filter(|e| e.event_type == event_type)
            .collect();

        if matching.len() >= min_count {
            return matching;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!(
                "Timed out waiting for >= {} '{}' events (got {})",
                min_count,
                event_type,
                matching.len()
            );
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
}

// ===========================================================================
// TC-259 — Metric aggregator emits MetricRecorded events every 15 seconds
// ===========================================================================

/// TC-259: Scenario test — The OtelAggregator subscribes to the OtelStream,
/// aggregates OTel metric data points, and emits MetricRecorded events to the
/// platform event log at a regular interval.
///
/// Steps:
/// 1. Create an OtelStream, event log, and OtelAggregator with a 1-second interval
/// 2. Publish several OTel metric data points to the stream
/// 3. Wait for the first aggregation interval to fire
/// 4. Verify a MetricRecorded event was emitted to the event log
/// 5. Verify the event payload contains correct aggregated metric values
/// 6. Publish more metrics and verify a second MetricRecorded event arrives
/// 7. Verify that when no metrics are published, no MetricRecorded event is emitted
#[tokio::test]
async fn tc259_metric_aggregator_emits_metricrecorded_events_every_15_seconds() {
    let (otel_stream, event_log, node_iri) = test_infra();
    let telemetry_store = make_telemetry_store();

    // Step 1: Create the aggregator with 1s interval (fast for testing)
    let aggregator = OtelAggregator::new(
        otel_stream.clone(),
        event_log.clone(),
        telemetry_store,
        new_iri_builder(),
        node_iri.clone(),
    );
    let _handle = aggregator.start(1); // 1-second interval for test speed

    // Step 2: Publish OTel metric data points
    otel_stream.publish(OtelDatum::Metric(make_metric(
        "http_request_duration_ms",
        100.0,
        "ms",
        "api-server",
    )));
    otel_stream.publish(OtelDatum::Metric(make_metric(
        "http_request_duration_ms",
        200.0,
        "ms",
        "api-server",
    )));
    otel_stream.publish(OtelDatum::Metric(make_metric(
        "db_query_time_ms",
        50.0,
        "ms",
        "db-proxy",
    )));

    // Small delay to ensure collector task receives all metrics before aggregation
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Step 3-4: Wait for a MetricRecorded event
    let metric_events = wait_for_events(&event_log, "MetricRecorded", 1, 5000).await;

    assert!(
        !metric_events.is_empty(),
        "At least one MetricRecorded event should be emitted"
    );

    // Step 5: Verify the payload contains correct aggregated metric values
    let first_event = &metric_events[0];
    assert_eq!(first_event.event_type, "MetricRecorded");

    let payload: MetricRecordedPayload =
        serde_json::from_value(first_event.payload.clone()).expect("Payload should deserialize");

    assert_eq!(
        payload.node_iri, node_iri,
        "MetricRecorded should reference the correct node"
    );
    assert!(
        !payload.metrics.is_empty(),
        "MetricRecorded should contain aggregated metrics"
    );

    // Should have two distinct metrics: http_request_duration_ms and db_query_time_ms
    let http_metric = payload
        .metrics
        .iter()
        .find(|m| m.name == "http_request_duration_ms")
        .expect("Should contain http_request_duration_ms metric");
    // Mean of 100.0 and 200.0 = 150.0
    assert!(
        (http_metric.value - 150.0).abs() < f64::EPSILON,
        "http_request_duration_ms should be mean of (100, 200) = 150.0, got {}",
        http_metric.value
    );
    assert_eq!(http_metric.unit, "ms");

    let db_metric = payload
        .metrics
        .iter()
        .find(|m| m.name == "db_query_time_ms")
        .expect("Should contain db_query_time_ms metric");
    assert!(
        (db_metric.value - 50.0).abs() < f64::EPSILON,
        "db_query_time_ms should be 50.0, got {}",
        db_metric.value
    );

    // Step 6: Publish more metrics and verify a second event arrives
    otel_stream.publish(OtelDatum::Metric(make_metric(
        "active_connections",
        42.0,
        "count",
        "api-server",
    )));

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let metric_events = wait_for_events(&event_log, "MetricRecorded", 2, 5000).await;
    assert!(
        metric_events.len() >= 2,
        "Should have at least 2 MetricRecorded events after second interval"
    );

    let second_payload: MetricRecordedPayload =
        serde_json::from_value(metric_events[1].payload.clone())
            .expect("Second payload should deserialize");
    let conn_metric = second_payload
        .metrics
        .iter()
        .find(|m| m.name == "active_connections")
        .expect("Second event should contain active_connections metric");
    assert!(
        (conn_metric.value - 42.0).abs() < f64::EPSILON,
        "active_connections should be 42.0"
    );

    // Step 7: Let the next interval fire without publishing any metrics.
    // Count current MetricRecorded events.
    let current_count = event_log
        .events_since(0)
        .await
        .iter()
        .filter(|e| e.event_type == "MetricRecorded")
        .count();

    // Wait a bit longer than the interval to confirm no spurious event
    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    let after_count = event_log
        .events_since(0)
        .await
        .iter()
        .filter(|e| e.event_type == "MetricRecorded")
        .count();

    assert_eq!(
        after_count, current_count,
        "No new MetricRecorded event should be emitted when no OTel metrics arrive"
    );
}

// ===========================================================================
// TC-316 — Metric aggregator exit — MetricRecorded events emitted on schedule
// ===========================================================================

/// TC-316: Exit-criteria test — validates end-to-end that OTel metric data
/// flowing through the OtelStream is aggregated into MetricRecorded platform
/// events on the expected schedule.
///
/// This is the verification that the full data path works:
///   OtelStream.publish(Metric) → collector task → metric buffer →
///   aggregator task → EventLog.append(MetricRecorded)
///
/// Steps:
/// 1. Create the full aggregation pipeline (OtelStream + OtelAggregator + EventLog)
/// 2. Publish diverse OTel metrics over two aggregation windows
/// 3. Verify MetricRecorded events appear on schedule (one per interval)
/// 4. Verify each event contains the correct time-windowed aggregates
/// 5. Verify TelemetryAggregated events are also emitted (co-existence)
/// 6. Verify MetricRecorded event schema IRI is correctly formed
#[tokio::test]
async fn tc316_metric_aggregator_exit_metricrecorded_events_emitted_on_schedule() {
    let (otel_stream, event_log, node_iri) = test_infra();
    let iri_builder = new_iri_builder();
    let telemetry_store = make_telemetry_store();

    // Step 1: Create and start the aggregator with 1s interval
    let aggregator = OtelAggregator::new(
        otel_stream.clone(),
        event_log.clone(),
        telemetry_store,
        new_iri_builder(),
        node_iri.clone(),
    );
    let _handle = aggregator.start(1);

    // Step 2: Window 1 — publish metrics
    otel_stream.publish(OtelDatum::Metric(make_metric(
        "request_count",
        10.0,
        "count",
        "frontend",
    )));
    otel_stream.publish(OtelDatum::Metric(make_metric(
        "request_count",
        20.0,
        "count",
        "frontend",
    )));
    otel_stream.publish(OtelDatum::Metric(make_metric(
        "error_rate",
        0.05,
        "ratio",
        "frontend",
    )));

    // Also publish a span so TelemetryAggregated fires too
    otel_stream.publish(OtelDatum::Span(picloud_domain::events::SpanRecord {
        trace_id: "t-316-1".to_string(),
        span_id: "s-316-1".to_string(),
        parent_span_id: None,
        operation_name: "GET /".to_string(),
        service_name: "frontend".to_string(),
        start_time: Utc::now(),
        end_time: Utc::now(),
        duration_ms: 10,
        status: "OK".to_string(),
        attributes: serde_json::json!({}),
    }));

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Step 3: Wait for first MetricRecorded event
    let metric_events = wait_for_events(&event_log, "MetricRecorded", 1, 5000).await;
    let first_event = &metric_events[0];

    // Step 4: Verify Window 1 aggregates
    let payload: MetricRecordedPayload =
        serde_json::from_value(first_event.payload.clone()).expect("Payload should deserialize");

    assert_eq!(payload.node_iri, node_iri, "Node IRI should match");

    let request_count = payload
        .metrics
        .iter()
        .find(|m| m.name == "request_count")
        .expect("Should contain request_count");
    // Mean of 10 and 20 = 15
    assert!(
        (request_count.value - 15.0).abs() < f64::EPSILON,
        "request_count mean should be 15.0, got {}",
        request_count.value
    );

    let error_rate = payload
        .metrics
        .iter()
        .find(|m| m.name == "error_rate")
        .expect("Should contain error_rate");
    assert!(
        (error_rate.value - 0.05).abs() < f64::EPSILON,
        "error_rate should be 0.05"
    );
    assert_eq!(error_rate.unit, "ratio");

    // Window 2 — publish different metrics
    otel_stream.publish(OtelDatum::Metric(make_metric(
        "response_time_ms",
        250.0,
        "ms",
        "backend",
    )));

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let metric_events = wait_for_events(&event_log, "MetricRecorded", 2, 5000).await;
    assert!(
        metric_events.len() >= 2,
        "Should have at least 2 MetricRecorded events across two windows"
    );

    let second_payload: MetricRecordedPayload =
        serde_json::from_value(metric_events[1].payload.clone())
            .expect("Second payload should deserialize");

    let response_time = second_payload
        .metrics
        .iter()
        .find(|m| m.name == "response_time_ms")
        .expect("Window 2 should contain response_time_ms");
    assert!(
        (response_time.value - 250.0).abs() < f64::EPSILON,
        "response_time_ms should be 250.0"
    );

    // The second window should NOT contain Window 1 metrics (buffer was drained)
    assert!(
        second_payload
            .metrics
            .iter()
            .find(|m| m.name == "request_count")
            .is_none(),
        "Window 2 should not contain Window 1 metrics (buffer is drained each interval)"
    );

    // Step 5: Verify TelemetryAggregated events are also emitted
    let telemetry_events = event_log
        .events_since(0)
        .await
        .into_iter()
        .filter(|e| e.event_type == "TelemetryAggregated")
        .collect::<Vec<_>>();

    assert!(
        !telemetry_events.is_empty(),
        "TelemetryAggregated events should also be emitted alongside MetricRecorded"
    );

    // Step 6: Verify MetricRecorded event schema IRI
    let expected_schema = iri_builder.event_schema("MetricRecorded", 1);
    assert_eq!(
        first_event.schema, expected_schema,
        "MetricRecorded schema IRI should be correctly formed"
    );

    // Verify source IRI matches the node
    assert_eq!(
        first_event.source, node_iri,
        "MetricRecorded source should be the node IRI"
    );

    // Verify it's a platform-level event (no product scope)
    assert!(
        first_event.product.is_none(),
        "MetricRecorded from OTel aggregator should be platform-level (no product scope)"
    );
}
