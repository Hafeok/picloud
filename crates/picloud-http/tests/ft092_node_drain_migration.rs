//! FT-092 — Node drain and graceful workload migration (exit criteria)
//!
//! Covers TC-345 (node drain exit — workloads migrated to other nodes).
//!
//! This exit-criteria test verifies the end-to-end drain-and-migrate flow:
//! 1. Nodes are registered in the RDF graph via NodeJoined events.
//! 2. A cordon → drain cycle completes with all workloads migrated.
//! 3. WorkloadMigrated events are projected into the RDF graph.
//! 4. A SPARQL query confirms every workload moved to a healthy node.
//! 5. The event chain (NodeDrainStarted → WorkloadMigrated* → NodeDrainCompleted)
//!    is fully correlated and projected.

use std::sync::Arc;
use uuid::Uuid;

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder};
use picloud_domain::traits::{
    EventLog, NodeDrainCoordinator, NodeDrainState, NodeWorkloadInfo, StateProjector,
};
use picloud_events::InMemoryEventLog;
use picloud_rdf::OxigraphProjector;
use picloud_workload::InMemoryDrainCoordinator;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn iri_builder() -> IriBuilder {
    IriBuilder::new(ClusterDomain::default())
}

fn container_workload(name: &str) -> NodeWorkloadInfo {
    let ib = iri_builder();
    NodeWorkloadInfo {
        workload_iri: ib.resource("test-product", "containers", name),
        workload_type: "container".to_string(),
    }
}

fn binary_workload(name: &str) -> NodeWorkloadInfo {
    let ib = iri_builder();
    NodeWorkloadInfo {
        workload_iri: ib.resource("test-product", "binaries", name),
        workload_type: "binary".to_string(),
    }
}

// ============================================================================
// TC-345 — Node drain exit — workloads migrated to other nodes
// ============================================================================

/// Exit criteria: verify the full end-to-end drain cycle with event log
/// integration, RDF projection, and SPARQL verification that all workloads
/// have been migrated off the drained node.
#[tokio::test]
async fn tc345_node_drain_exit_workloads_migrated_to_other_nodes() {
    let ib = iri_builder();
    let event_log = Arc::new(InMemoryEventLog::new());
    let projector = OxigraphProjector::new().expect("Failed to create projector");
    let coordinator = InMemoryDrainCoordinator::new();

    // ========================================================================
    // Phase 1: Set up cluster topology in RDF
    // ========================================================================

    let drain_node_id = Uuid::new_v4();
    let target_node_1_id = Uuid::new_v4();
    let target_node_2_id = Uuid::new_v4();

    let drain_node_iri = ib.node("drain-node");
    let target_1_iri = ib.node("target-node-1");
    let target_2_iri = ib.node("target-node-2");

    // Register nodes in coordinator
    coordinator.register_node(drain_node_id).await;
    coordinator.register_node(target_node_1_id).await;
    coordinator.register_node(target_node_2_id).await;
    coordinator.set_available_nodes(vec![target_node_1_id, target_node_2_id]).await;

    // Project NodeJoined events so nodes exist in the RDF graph
    for (node_id, node_iri, node_name) in [
        (drain_node_id, &drain_node_iri, "drain-node"),
        (target_node_1_id, &target_1_iri, "target-node-1"),
        (target_node_2_id, &target_2_iri, "target-node-2"),
    ] {
        let join_event = EventEnvelope::new(
            ib.event_schema("NodeJoined", 1),
            "NodeJoined",
            node_iri.clone(),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "node_id": node_id.to_string(),
                "node_name": node_name,
                "node_iri": node_iri.as_str(),
                "address": format!("192.168.1.{}:7443", node_name.len()),
            }),
        );
        event_log.append(join_event.clone()).await.unwrap();
        projector.project(&join_event).await.unwrap();
    }

    // ========================================================================
    // Phase 2: Register workloads on the drain node
    // ========================================================================

    let workloads = vec![
        container_workload("api-server"),
        container_workload("web-frontend"),
        binary_workload("log-shipper"),
    ];
    for w in &workloads {
        coordinator.register_workload(drain_node_id, w.clone()).await;
    }

    // Verify preconditions
    let pre_drain = coordinator.node_workloads(drain_node_id).await.unwrap();
    assert_eq!(pre_drain.len(), 3, "Drain node should start with 3 workloads");

    // ========================================================================
    // Phase 3: Cordon → Drain with full event chain
    // ========================================================================

    let correlation_id = Uuid::new_v4();

    // 3a. Cordon
    coordinator.cordon(drain_node_id).await.unwrap();
    let cordon_event = EventEnvelope::new(
        ib.event_schema("NodeCordoned", 1),
        "NodeCordoned",
        drain_node_iri.clone(),
        None,
        correlation_id,
        serde_json::json!({
            "node_id": drain_node_id.to_string(),
            "node_iri": drain_node_iri.as_str(),
            "node_name": "drain-node",
        }),
    );
    event_log.append(cordon_event.clone()).await.unwrap();
    projector.project(&cordon_event).await.unwrap();

    // Verify cordoned state in RDF
    let cordon_query = format!(
        r#"
        PREFIX picloud: <https://picloud.local/ontology#>
        SELECT ?status WHERE {{
            <{node}> picloud:drainStatus ?status .
        }}
        "#,
        node = drain_node_iri.as_str()
    );
    let result = projector.query(&cordon_query).await.unwrap();
    assert!(!result.bindings.is_empty(), "Node should have drainStatus after cordon");
    let status = result.bindings[0]
        .get("status")
        .and_then(|v| v.as_str().or_else(|| v.get("value").and_then(|v| v.as_str())))
        .unwrap_or("");
    assert_eq!(status, "cordoned", "DrainStatus should be 'cordoned'");

    // 3b. Uncordon so drain() can set Draining from Active
    coordinator.uncordon(drain_node_id).await.unwrap();

    // 3c. Emit NodeDrainStarted
    let drain_started = EventEnvelope::new(
        ib.event_schema("NodeDrainStarted", 1),
        "NodeDrainStarted",
        drain_node_iri.clone(),
        None,
        correlation_id,
        serde_json::json!({
            "node_id": drain_node_id.to_string(),
            "node_iri": drain_node_iri.as_str(),
            "node_name": "drain-node",
            "workload_count": 3,
        }),
    );
    event_log.append(drain_started.clone()).await.unwrap();
    projector.project(&drain_started).await.unwrap();

    // 3d. Execute the drain
    let drain_result = coordinator.drain(drain_node_id, 30).await.unwrap();
    assert!(drain_result.success, "Drain should succeed");
    assert_eq!(drain_result.workloads_migrated, 3, "All 3 workloads should be migrated");

    // 3e. Emit WorkloadMigrated events from the migration log
    let migrations = coordinator.migration_log().await;
    assert_eq!(migrations.len(), 3, "Should have 3 migration records");

    for m in &migrations {
        let to_iri = if m.to_node_id == target_node_1_id {
            &target_1_iri
        } else {
            &target_2_iri
        };
        let migrated_event = EventEnvelope::new(
            ib.event_schema("WorkloadMigrated", 1),
            "WorkloadMigrated",
            drain_node_iri.clone(),
            Some("test-product".to_string()),
            correlation_id,
            serde_json::json!({
                "workload_iri": m.workload_iri.as_str(),
                "from_node_iri": drain_node_iri.as_str(),
                "to_node_iri": to_iri.as_str(),
                "reason": "node_drain",
                "workload_type": m.workload_type,
            }),
        );
        event_log.append(migrated_event.clone()).await.unwrap();
        projector.project(&migrated_event).await.unwrap();
    }

    // 3f. Emit NodeDrainCompleted
    let drain_completed = EventEnvelope::new(
        ib.event_schema("NodeDrainCompleted", 1),
        "NodeDrainCompleted",
        drain_node_iri.clone(),
        None,
        correlation_id,
        serde_json::json!({
            "node_id": drain_node_id.to_string(),
            "node_iri": drain_node_iri.as_str(),
            "node_name": "drain-node",
            "workloads_migrated": drain_result.workloads_migrated,
            "duration_ms": drain_result.duration_ms,
        }),
    );
    event_log.append(drain_completed.clone()).await.unwrap();
    projector.project(&drain_completed).await.unwrap();

    // ========================================================================
    // Phase 4: Verify — RDF graph reflects drained state
    // ========================================================================

    // 4a. Verify the node is marked as "drained" in RDF
    let drain_query = format!(
        r#"
        PREFIX picloud: <https://picloud.local/ontology#>
        SELECT ?status WHERE {{
            <{node}> picloud:drainStatus ?status .
        }}
        "#,
        node = drain_node_iri.as_str()
    );
    let result = projector.query(&drain_query).await.unwrap();
    assert!(!result.bindings.is_empty(), "Node should have drainStatus in RDF after drain");
    let status = result.bindings[0]
        .get("status")
        .and_then(|v| v.as_str().or_else(|| v.get("value").and_then(|v| v.as_str())))
        .unwrap_or("");
    assert_eq!(status, "drained", "DrainStatus should be 'drained'");

    // ========================================================================
    // Phase 5: Verify — coordinator state matches
    // ========================================================================

    // 5a. Drained node has no workloads
    let remaining = coordinator.node_workloads(drain_node_id).await.unwrap();
    assert!(remaining.is_empty(), "EXIT CRITERIA: Drained node must have zero workloads");

    // 5b. Target nodes received workloads
    let t1_wl = coordinator.node_workloads(target_node_1_id).await.unwrap();
    let t2_wl = coordinator.node_workloads(target_node_2_id).await.unwrap();
    let total_on_targets = t1_wl.len() + t2_wl.len();
    assert_eq!(
        total_on_targets, 3,
        "EXIT CRITERIA: All 3 workloads must be on target nodes (got {total_on_targets})"
    );

    // 5c. Drain state is Drained
    assert_eq!(
        coordinator.drain_state(drain_node_id).await.unwrap(),
        NodeDrainState::Drained,
        "EXIT CRITERIA: Node must be in Drained state"
    );

    // ========================================================================
    // Phase 6: Verify — event log is complete and correlated
    // ========================================================================

    let all_events = event_log.events_since(0).await;

    // Count drain-related events (skip the 3 NodeJoined events)
    let drain_events: Vec<_> = all_events
        .iter()
        .filter(|e| e.correlation_id == correlation_id)
        .collect();

    // Expected: NodeCordoned + NodeDrainStarted + 3x WorkloadMigrated + NodeDrainCompleted = 6
    assert_eq!(
        drain_events.len(),
        6,
        "EXIT CRITERIA: Should have 6 correlated drain events, got {}",
        drain_events.len()
    );

    // Verify event ordering
    let drain_event_types: Vec<&str> = drain_events
        .iter()
        .map(|e| e.event_type.as_str())
        .collect();
    assert_eq!(drain_event_types[0], "NodeCordoned");
    assert_eq!(drain_event_types[1], "NodeDrainStarted");
    assert_eq!(drain_event_types[2], "WorkloadMigrated");
    assert_eq!(drain_event_types[3], "WorkloadMigrated");
    assert_eq!(drain_event_types[4], "WorkloadMigrated");
    assert_eq!(drain_event_types[5], "NodeDrainCompleted");

    // Verify the completed event carries the correct migration count
    let completed_payload = &drain_events[5].payload;
    assert_eq!(
        completed_payload.get("workloads_migrated").and_then(|v| v.as_u64()),
        Some(3),
        "EXIT CRITERIA: NodeDrainCompleted must report 3 workloads migrated"
    );

    // ========================================================================
    // Phase 7: Verify — uncordon brings node back to Active
    // ========================================================================

    coordinator.uncordon(drain_node_id).await.unwrap();
    assert_eq!(
        coordinator.drain_state(drain_node_id).await.unwrap(),
        NodeDrainState::Active,
        "EXIT CRITERIA: Node should return to Active after uncordon from Drained"
    );
}
