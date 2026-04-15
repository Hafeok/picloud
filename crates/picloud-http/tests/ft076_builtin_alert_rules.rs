//! FT-076 — Built-in platform alert rules (CPU temp, memory, disk, node health, workload failure)
//!
//! Covers TC-123 (metrics_collection_interval), TC-124 (metrics_rdf_projection),
//! TC-125/TC-126 (metrics_upsert), TC-129 (alert_dampening), TC-130 (all_builtin_rules).
//!
//! These tests verify:
//! 1. The MetricsAgent collects metrics at the configured interval.
//! 2. MetricRecorded events are projected into the RDF graph correctly.
//! 3. Metric projection uses upsert semantics (only latest values stored).
//! 4. Alert dampening prevents re-firing within the dampening window.
//! 5. All built-in alert rules fire with correct types and severities.

use std::sync::Arc;

use picloud_domain::events::{AlertSeverity, MetricEntry};
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::resources::builtin_alert_rules;
use picloud_domain::traits::{AlertAction, AlertEvaluator, StateProjector};
use picloud_events::InMemoryEventLog;
use picloud_http::{BuiltInAlertEvaluator, MetricsAgent};
use picloud_rdf::OxigraphProjector;

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
// TC-123 — Metrics collection interval
// ============================================================================
/// Start a MetricsAgent with a 1-second interval and verify that at least 2
/// MetricRecorded events are emitted, each containing CPU usage, memory,
/// disk, and CPU temperature metrics.
#[tokio::test]
async fn metrics_collection_interval() {
    let (event_log, node_iri) = test_infra();

    let metric_source = || -> Vec<MetricEntry> {
        vec![
            MetricEntry {
                name: "cpu_usage_percent".to_string(),
                value: 42.3,
                unit: "percent".to_string(),
            },
            MetricEntry {
                name: "memory_used_mb".to_string(),
                value: 8192.0,
                unit: "mb".to_string(),
            },
            MetricEntry {
                name: "memory_total_mb".to_string(),
                value: 16384.0,
                unit: "mb".to_string(),
            },
            MetricEntry {
                name: "disk_used_gb".to_string(),
                value: 312.0,
                unit: "gb".to_string(),
            },
            MetricEntry {
                name: "disk_total_gb".to_string(),
                value: 1000.0,
                unit: "gb".to_string(),
            },
            MetricEntry {
                name: "cpu_temp_celsius".to_string(),
                value: 58.1,
                unit: "celsius".to_string(),
            },
        ]
    };

    let agent = MetricsAgent::new(event_log.clone(), new_iri_builder(), node_iri.clone());
    let handle = agent.start_with_source(1, metric_source);

    // Wait for at least 2 MetricRecorded events (proves periodic collection)
    let events = wait_for_events(&event_log, "MetricRecorded", 2, 5000).await;
    assert!(events.len() >= 2, "Expected at least 2 MetricRecorded events");

    // Verify payload structure: each event should contain all 6 metric types
    for event in &events {
        let payload = &event.payload;
        let metrics = payload["metrics"].as_array().expect("metrics should be an array");
        assert!(metrics.len() >= 6, "Expected at least 6 metrics per collection");

        let metric_names: Vec<&str> = metrics
            .iter()
            .filter_map(|m| m["name"].as_str())
            .collect();
        assert!(metric_names.contains(&"cpu_usage_percent"));
        assert!(metric_names.contains(&"memory_used_mb"));
        assert!(metric_names.contains(&"memory_total_mb"));
        assert!(metric_names.contains(&"disk_used_gb"));
        assert!(metric_names.contains(&"disk_total_gb"));
        assert!(metric_names.contains(&"cpu_temp_celsius"));
    }

    handle.abort();
}

// ============================================================================
// TC-124 — Metrics RDF projection
// ============================================================================
/// After a MetricRecorded event is projected, query the node IRI via SPARQL
/// and assert that picloud:cpuUsagePercent, picloud:memoryUsedMb,
/// picloud:memoryTotalMb, picloud:cpuTempCelsius, and picloud:metricsUpdatedAt
/// are present.
#[tokio::test]
async fn metrics_rdf_projection() {
    let ib = new_iri_builder();
    let node_iri = ib.node("test-node-01");
    let projector = OxigraphProjector::new().expect("Failed to create projector");

    // First project a NodeJoined event so the node exists in the graph
    let node_joined = picloud_domain::events::EventEnvelope::new(
        ib.event_schema("NodeJoined", 1),
        "NodeJoined",
        node_iri.clone(),
        None,
        uuid::Uuid::new_v4(),
        serde_json::json!({
            "node_id": uuid::Uuid::new_v4().to_string(),
            "node_name": "test-node-01",
            "node_iri": node_iri.as_str(),
            "address": "192.168.1.101",
        }),
    );
    projector.project(&node_joined).await.unwrap();

    // Project a MetricRecorded event
    let metric_event = picloud_domain::events::EventEnvelope::new(
        ib.event_schema("MetricRecorded", 1),
        "MetricRecorded",
        node_iri.clone(),
        None,
        uuid::Uuid::new_v4(),
        serde_json::json!({
            "node_iri": node_iri.as_str(),
            "metrics": [
                { "name": "cpu_usage_percent", "value": 42.3, "unit": "percent" },
                { "name": "memory_used_mb", "value": 8192.0, "unit": "mb" },
                { "name": "memory_total_mb", "value": 16384.0, "unit": "mb" },
                { "name": "disk_used_gb", "value": 312.0, "unit": "gb" },
                { "name": "disk_total_gb", "value": 1000.0, "unit": "gb" },
                { "name": "cpu_temp_celsius", "value": 58.1, "unit": "celsius" },
            ]
        }),
    );
    projector.project(&metric_event).await.unwrap();

    // Query for metric values on the node
    let query = format!(
        r#"
        PREFIX picloud: <https://picloud.local/ontology#>
        SELECT ?cpu ?memUsed ?memTotal ?temp ?updated
        WHERE {{
            <{node}> picloud:cpuUsagePercent ?cpu ;
                     picloud:memoryUsedMb ?memUsed ;
                     picloud:memoryTotalMb ?memTotal ;
                     picloud:cpuTempCelsius ?temp ;
                     picloud:metricsUpdatedAt ?updated .
        }}
        "#,
        node = node_iri.as_str()
    );

    let result = projector.query(&query).await.unwrap();
    assert!(
        !result.bindings.is_empty(),
        "Expected SPARQL results for metric triples on node IRI"
    );

    let row = &result.bindings[0];
    // Verify the metric values are present
    assert!(row.get("cpu").is_some(), "cpuUsagePercent should be present");
    assert!(row.get("memUsed").is_some(), "memoryUsedMb should be present");
    assert!(row.get("memTotal").is_some(), "memoryTotalMb should be present");
    assert!(row.get("temp").is_some(), "cpuTempCelsius should be present");
    assert!(row.get("updated").is_some(), "metricsUpdatedAt should be present");
}

// ============================================================================
// TC-125 / TC-126 — Metrics upsert (only latest values stored)
// ============================================================================
/// Wait for two consecutive MetricRecorded events from the same node and
/// assert the graph holds only the latest metric values (not a growing list
/// of historical values).
#[tokio::test]
async fn metrics_upsert() {
    let ib = new_iri_builder();
    let node_iri = ib.node("test-node-01");
    let projector = OxigraphProjector::new().expect("Failed to create projector");

    // Project a NodeJoined event first
    let node_joined = picloud_domain::events::EventEnvelope::new(
        ib.event_schema("NodeJoined", 1),
        "NodeJoined",
        node_iri.clone(),
        None,
        uuid::Uuid::new_v4(),
        serde_json::json!({
            "node_id": uuid::Uuid::new_v4().to_string(),
            "node_name": "test-node-01",
            "node_iri": node_iri.as_str(),
            "address": "192.168.1.101",
        }),
    );
    projector.project(&node_joined).await.unwrap();

    // Project first metric event with value 42.3
    let metric_1 = picloud_domain::events::EventEnvelope::new(
        ib.event_schema("MetricRecorded", 1),
        "MetricRecorded",
        node_iri.clone(),
        None,
        uuid::Uuid::new_v4(),
        serde_json::json!({
            "node_iri": node_iri.as_str(),
            "metrics": [
                { "name": "cpu_temp_celsius", "value": 42.3, "unit": "celsius" },
            ]
        }),
    );
    projector.project(&metric_1).await.unwrap();

    // Project second metric event with value 58.1 (should overwrite)
    let metric_2 = picloud_domain::events::EventEnvelope::new(
        ib.event_schema("MetricRecorded", 1),
        "MetricRecorded",
        node_iri.clone(),
        None,
        uuid::Uuid::new_v4(),
        serde_json::json!({
            "node_iri": node_iri.as_str(),
            "metrics": [
                { "name": "cpu_temp_celsius", "value": 58.1, "unit": "celsius" },
            ]
        }),
    );
    projector.project(&metric_2).await.unwrap();

    // Query: there should be exactly ONE cpuTempCelsius triple (upsert, not append)
    let count_query = format!(
        r#"
        PREFIX picloud: <https://picloud.local/ontology#>
        SELECT (COUNT(?temp) AS ?count)
        WHERE {{
            <{node}> picloud:cpuTempCelsius ?temp .
        }}
        "#,
        node = node_iri.as_str()
    );

    let result = projector.query(&count_query).await.unwrap();
    assert!(!result.bindings.is_empty());
    let count_val = &result.bindings[0]["count"];
    // The count should be "1" regardless of how many MetricRecorded events were projected
    let count_str = count_val.as_str().unwrap_or(
        count_val.get("value").and_then(|v| v.as_str()).unwrap_or("0")
    );
    let count: i64 = count_str.parse().unwrap_or(0);
    assert_eq!(count, 1, "Should have exactly 1 cpuTempCelsius triple (upsert semantics)");

    // Also verify the value is the latest one (58.1, not 42.3)
    let value_query = format!(
        r#"
        PREFIX picloud: <https://picloud.local/ontology#>
        SELECT ?temp
        WHERE {{
            <{node}> picloud:cpuTempCelsius ?temp .
        }}
        "#,
        node = node_iri.as_str()
    );

    let result = projector.query(&value_query).await.unwrap();
    assert!(!result.bindings.is_empty());
    let temp_val = &result.bindings[0]["temp"];
    let temp_str = temp_val.as_str().unwrap_or(
        temp_val.get("value").and_then(|v| v.as_str()).unwrap_or("0")
    );
    let temp: f64 = temp_str.parse().unwrap_or(0.0);
    assert!(
        (temp - 58.1).abs() < 0.01,
        "Expected latest temperature 58.1, got {temp}"
    );
}

// ============================================================================
// TC-129 — Alert dampening
// ============================================================================
/// Fire an alert, resolve it, re-fire within the dampening window and verify
/// it is suppressed. Then wait past the dampening window and verify the alert
/// fires again.
#[tokio::test]
async fn alert_dampening() {
    let ib = new_iri_builder();
    let node_iri = ib.node("test-node-01");

    // Use a 2-second dampening window for test speed
    let evaluator = BuiltInAlertEvaluator::new(builtin_alert_rules(), new_iri_builder())
        .with_dampening_secs(2);

    let hot_metrics = vec![MetricEntry {
        name: "cpu_temp_celsius".to_string(),
        value: 85.0,
        unit: "celsius".to_string(),
    }];
    let cool_metrics = vec![MetricEntry {
        name: "cpu_temp_celsius".to_string(),
        value: 55.0,
        unit: "celsius".to_string(),
    }];

    // Step 1: Fire the alert
    let actions = evaluator.evaluate(&node_iri, &hot_metrics).await.unwrap();
    let fired: Vec<_> = actions
        .iter()
        .filter(|a| matches!(a, AlertAction::Fire(_)))
        .collect();
    assert!(
        !fired.is_empty(),
        "Should fire alerts on first threshold breach"
    );

    // Step 2: Resolve the alert
    let actions = evaluator.evaluate(&node_iri, &cool_metrics).await.unwrap();
    let resolved: Vec<_> = actions
        .iter()
        .filter(|a| matches!(a, AlertAction::Resolve(_)))
        .collect();
    assert!(
        !resolved.is_empty(),
        "Should resolve alerts when temp drops"
    );

    // Step 3: Immediately re-trigger (within dampening window) — should be suppressed
    let actions = evaluator.evaluate(&node_iri, &hot_metrics).await.unwrap();
    let re_fired: Vec<_> = actions
        .iter()
        .filter(|a| matches!(a, AlertAction::Fire(_)))
        .collect();
    assert!(
        re_fired.is_empty(),
        "Alert should be dampened within the dampening window"
    );

    // Step 4: Wait past the dampening window (2 seconds + small buffer)
    tokio::time::sleep(tokio::time::Duration::from_millis(2200)).await;

    // Step 5: Re-trigger — should fire now
    let actions = evaluator.evaluate(&node_iri, &hot_metrics).await.unwrap();
    let re_fired: Vec<_> = actions
        .iter()
        .filter(|a| matches!(a, AlertAction::Fire(_)))
        .collect();
    assert!(
        !re_fired.is_empty(),
        "Alert should fire again after dampening window expires"
    );
}

// ============================================================================
// TC-130 — All built-in rules
// ============================================================================
/// For each built-in alert rule (CPU temp warning/critical, memory
/// warning/critical, disk critical), trigger the threshold condition and
/// assert the correct AlertFired event type and severity.
#[tokio::test]
async fn all_builtin_rules() {
    let ib = new_iri_builder();
    let node_iri = ib.node("test-node-01");
    let evaluator = BuiltInAlertEvaluator::new(builtin_alert_rules(), new_iri_builder());

    // --- CPU Temperature: 85 C exceeds both 70 C (warning) and 80 C (critical) ---
    let cpu_temp_metrics = vec![MetricEntry {
        name: "cpu_temp_celsius".to_string(),
        value: 85.0,
        unit: "celsius".to_string(),
    }];

    let actions = evaluator.evaluate(&node_iri, &cpu_temp_metrics).await.unwrap();
    let cpu_fired: Vec<_> = actions
        .iter()
        .filter_map(|a| match a {
            AlertAction::Fire(p) => Some(p),
            _ => None,
        })
        .collect();

    // Both warning and critical should fire
    assert_eq!(cpu_fired.len(), 2, "CPU temp should trigger 2 alerts (warning + critical)");
    assert!(
        cpu_fired.iter().any(|p| p.alert_type == "HighCpuTemperature" && p.severity == AlertSeverity::Warning),
        "Expected HighCpuTemperature warning alert"
    );
    assert!(
        cpu_fired.iter().any(|p| p.alert_type == "HighCpuTemperature" && p.severity == AlertSeverity::Critical),
        "Expected HighCpuTemperature critical alert"
    );

    // Resolve CPU temp alerts before testing other rules
    let cool_metrics = vec![MetricEntry {
        name: "cpu_temp_celsius".to_string(),
        value: 55.0,
        unit: "celsius".to_string(),
    }];
    evaluator.evaluate(&node_iri, &cool_metrics).await.unwrap();

    // --- Memory: 95% exceeds both 80% (warning) and 90% (critical) ---
    // Use a fresh evaluator for memory to avoid dampening interactions
    let evaluator = BuiltInAlertEvaluator::new(builtin_alert_rules(), new_iri_builder());
    let memory_metrics = vec![MetricEntry {
        name: "memory_used_percent".to_string(),
        value: 95.0,
        unit: "percent".to_string(),
    }];

    let actions = evaluator.evaluate(&node_iri, &memory_metrics).await.unwrap();
    let mem_fired: Vec<_> = actions
        .iter()
        .filter_map(|a| match a {
            AlertAction::Fire(p) if p.alert_type == "HighMemoryUsage" => Some(p),
            _ => None,
        })
        .collect();

    assert_eq!(mem_fired.len(), 2, "Memory should trigger 2 alerts (warning + critical)");
    assert!(
        mem_fired.iter().any(|p| p.severity == AlertSeverity::Warning),
        "Expected HighMemoryUsage warning alert"
    );
    assert!(
        mem_fired.iter().any(|p| p.severity == AlertSeverity::Critical),
        "Expected HighMemoryUsage critical alert"
    );

    // --- Disk: 95% exceeds 90% (critical only — no warning rule) ---
    let disk_metrics = vec![MetricEntry {
        name: "disk_used_percent".to_string(),
        value: 95.0,
        unit: "percent".to_string(),
    }];

    let actions = evaluator.evaluate(&node_iri, &disk_metrics).await.unwrap();
    let disk_fired: Vec<_> = actions
        .iter()
        .filter_map(|a| match a {
            AlertAction::Fire(p) if p.alert_type == "HighDiskUsage" => Some(p),
            _ => None,
        })
        .collect();

    assert_eq!(disk_fired.len(), 1, "Disk should trigger 1 alert (critical only)");
    assert_eq!(disk_fired[0].severity, AlertSeverity::Critical);
    assert_eq!(disk_fired[0].alert_type, "HighDiskUsage");

    // --- Verify correct resource IRI on all alerts ---
    for action in &actions {
        if let AlertAction::Fire(p) = action {
            assert_eq!(
                p.resource_iri, node_iri,
                "Alert resource IRI should match the node IRI"
            );
        }
    }
}
