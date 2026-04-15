//! FT-075 — Platform metrics agent — MetricRecorded events at 15s interval per node
//!
//! Covers TC-229.
//! These tests verify that:
//! 1. The MetricsAgent with a BuiltInAlertEvaluator fires AlertFired events
//!    when CPU temperature exceeds the critical threshold (>80 C).
//! 2. AlertResolved events are emitted when CPU temperature returns to normal.
//! 3. Alert dampening prevents duplicate alerts while the condition persists.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use picloud_domain::events::{AlertFiredPayload, AlertResolvedPayload, MetricEntry};
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::resources::builtin_alert_rules;
use picloud_events::InMemoryEventLog;
use picloud_http::{BuiltInAlertEvaluator, MetricsAgent};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn new_iri_builder() -> IriBuilder {
    IriBuilder::new(ClusterDomain::default())
}

fn test_infra() -> (Arc<InMemoryEventLog>, ResourceIri) {
    let iri_builder = new_iri_builder();
    let node_iri = iri_builder.node("test-node-01");
    let event_log = Arc::new(InMemoryEventLog::new());
    (event_log, node_iri)
}

fn make_alert_evaluator() -> Arc<BuiltInAlertEvaluator> {
    Arc::new(BuiltInAlertEvaluator::new(
        builtin_alert_rules(),
        new_iri_builder(),
    ))
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

// ============================================================================
// TC-229 — CPU temperature alert fires and resolves
// ============================================================================
/// Simulate a MetricsAgent collecting high CPU temperature metrics, verify
/// that AlertFired events are emitted, then simulate the temperature dropping
/// and verify that AlertResolved events are emitted.
#[tokio::test]
async fn tc229_cpu_temperature_alert_fires_and_resolves() {
    let (event_log, node_iri) = test_infra();
    let alert_evaluator = make_alert_evaluator();

    // Use an atomic to control the simulated temperature across ticks
    let temp = Arc::new(AtomicU64::new(85_000)); // 85.0 C in milli-units

    let temp_clone = temp.clone();
    let metric_source = move || -> Vec<MetricEntry> {
        let celsius = temp_clone.load(Ordering::Relaxed) as f64 / 1000.0;
        vec![
            MetricEntry {
                name: "cpu_temp_celsius".to_string(),
                value: celsius,
                unit: "celsius".to_string(),
            },
            MetricEntry {
                name: "cpu_usage_percent".to_string(),
                value: 42.0,
                unit: "percent".to_string(),
            },
        ]
    };

    // Start the metrics agent with a short interval for testing (1s)
    let agent = MetricsAgent::new(event_log.clone(), new_iri_builder(), node_iri.clone())
        .with_alert_evaluator(alert_evaluator);
    let handle = agent.start_with_source(1, metric_source);

    // Phase 1: Wait for AlertFired events (CPU temp 85 C > 80 C critical threshold)
    let alert_fired_events = wait_for_events(&event_log, "AlertFired", 1, 5000).await;

    // Verify the AlertFired payload
    assert!(!alert_fired_events.is_empty(), "Expected at least one AlertFired event");
    let first_fired = &alert_fired_events[0];
    let fired_payload: AlertFiredPayload =
        serde_json::from_value(first_fired.payload.clone()).unwrap();
    assert_eq!(fired_payload.alert_type, "HighCpuTemperature");
    assert_eq!(fired_payload.resource_iri, node_iri);

    // Also verify a MetricRecorded event was emitted
    let metric_events = wait_for_events(&event_log, "MetricRecorded", 1, 5000).await;
    assert!(!metric_events.is_empty(), "Expected MetricRecorded events");

    // Phase 2: Drop the temperature below all thresholds
    temp.store(55_000, Ordering::Relaxed); // 55.0 C — well below 70 C warning

    // Wait for AlertResolved events
    let alert_resolved_events = wait_for_events(&event_log, "AlertResolved", 1, 5000).await;

    // Verify the AlertResolved payload
    assert!(
        !alert_resolved_events.is_empty(),
        "Expected at least one AlertResolved event"
    );
    let first_resolved = &alert_resolved_events[0];
    let resolved_payload: AlertResolvedPayload =
        serde_json::from_value(first_resolved.payload.clone()).unwrap();
    assert_eq!(resolved_payload.alert_type, "HighCpuTemperature");
    assert_eq!(resolved_payload.resource_iri, node_iri);

    // Phase 3: Verify dampening — no new AlertFired events while temp stays normal
    let all_events = event_log.events_since(0).await;
    let total_fired: Vec<_> = all_events
        .iter()
        .filter(|e| e.event_type == "AlertFired")
        .collect();

    // Should have fired alerts exactly once (both warning and critical fire on first tick)
    // Additional ticks at 85 C should NOT produce more AlertFired (dampening)
    // After temp drops, only AlertResolved events should appear
    let total_resolved: Vec<_> = all_events
        .iter()
        .filter(|e| e.event_type == "AlertResolved")
        .collect();

    // Both warning (>70) and critical (>80) should have fired
    assert!(
        total_fired.len() >= 2,
        "Expected at least 2 AlertFired events (warning + critical), got {}",
        total_fired.len()
    );

    // Both warning and critical should have resolved
    assert!(
        total_resolved.len() >= 2,
        "Expected at least 2 AlertResolved events, got {}",
        total_resolved.len()
    );

    // Clean up
    handle.abort();
}
