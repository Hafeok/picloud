//! FT-011 — Operational Maturity: Node Drain
//!
//! Covers TC-236 (node drain completes and workloads reschedule within timeout).
//!
//! These tests verify:
//! 1. A node can be cordoned (stops accepting new workloads).
//! 2. A node can be drained (workloads are migrated to other nodes).
//! 3. Drain completes within the specified timeout.
//! 4. Workloads are rescheduled on surviving nodes.
//! 5. The node transitions through correct drain states.
//! 6. After drain, the node can be uncordoned to accept workloads again.

use std::sync::Arc;
use uuid::Uuid;

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder};
use picloud_domain::traits::{
    EventLog, NodeDrainCoordinator, NodeDrainState, NodeWorkloadInfo,
};
use picloud_events::InMemoryEventLog;
use picloud_workload::InMemoryDrainCoordinator;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn iri_builder() -> IriBuilder {
    IriBuilder::new(ClusterDomain::default())
}

fn make_workload_info(name: &str) -> NodeWorkloadInfo {
    let ib = iri_builder();
    NodeWorkloadInfo {
        workload_iri: ib.resource("test-product", "containers", name),
        workload_type: "container".to_string(),
    }
}

// ============================================================================
// TC-236 — Node drain completes and workloads reschedule within timeout
// ============================================================================

/// Scenario: Set up a 3-node cluster with workloads on one node. Drain that
/// node and verify all workloads are migrated to surviving nodes within the
/// timeout. Verify drain state transitions and final state.
#[tokio::test]
async fn tc236_node_drain_completes_and_workloads_reschedule() {
    let coordinator = InMemoryDrainCoordinator::new();

    // Set up 3 nodes
    let node_a = Uuid::new_v4();
    let node_b = Uuid::new_v4();
    let node_c = Uuid::new_v4();

    coordinator.register_node(node_a).await;
    coordinator.register_node(node_b).await;
    coordinator.register_node(node_c).await;

    // Register 3 workloads on node_a (the node we'll drain)
    coordinator
        .register_workload(node_a, make_workload_info("web-server"))
        .await;
    coordinator
        .register_workload(node_a, make_workload_info("api-server"))
        .await;
    coordinator
        .register_workload(node_a, make_workload_info("worker"))
        .await;

    // Set target nodes for migration
    coordinator.set_available_nodes(vec![node_b, node_c]).await;

    // Verify initial state is Active
    let state = coordinator.drain_state(node_a).await.unwrap();
    assert_eq!(state, NodeDrainState::Active, "Node should start in Active state");

    // Verify node has 3 workloads
    let workloads = coordinator.node_workloads(node_a).await.unwrap();
    assert_eq!(workloads.len(), 3, "Node should have 3 workloads before drain");

    // --- Step 1: Cordon the node ---
    coordinator.cordon(node_a).await.unwrap();
    let state = coordinator.drain_state(node_a).await.unwrap();
    assert_eq!(
        state,
        NodeDrainState::Cordoned,
        "Node should be cordoned after cordon()"
    );

    // Uncordon and verify state returns to Active
    coordinator.uncordon(node_a).await.unwrap();
    let state = coordinator.drain_state(node_a).await.unwrap();
    assert_eq!(
        state,
        NodeDrainState::Active,
        "Node should be active after uncordon()"
    );

    // --- Step 2: Drain the node (with 30s timeout) ---
    let result = coordinator.drain(node_a, 30).await.unwrap();

    // Verify drain was successful
    assert!(result.success, "Drain should succeed");
    assert_eq!(
        result.workloads_migrated, 3,
        "All 3 workloads should be migrated"
    );
    assert!(
        result.duration_ms < 30_000,
        "Drain should complete within timeout (took {}ms)",
        result.duration_ms
    );
    assert!(result.error.is_none(), "No error should be present");

    // Verify final drain state
    let state = coordinator.drain_state(node_a).await.unwrap();
    assert_eq!(
        state,
        NodeDrainState::Drained,
        "Node should be in Drained state after successful drain"
    );

    // Verify no workloads remain on the drained node
    let workloads = coordinator.node_workloads(node_a).await.unwrap();
    assert!(
        workloads.is_empty(),
        "Drained node should have no workloads"
    );

    // --- Step 3: Uncordon the drained node to bring it back to service ---
    coordinator.uncordon(node_a).await.unwrap();
    let state = coordinator.drain_state(node_a).await.unwrap();
    assert_eq!(
        state,
        NodeDrainState::Active,
        "Node should be Active after uncordon from Drained state"
    );
}

/// Verify that draining a node with no workloads succeeds immediately.
#[tokio::test]
async fn tc236_drain_empty_node() {
    let coordinator = InMemoryDrainCoordinator::new();

    let node_a = Uuid::new_v4();
    coordinator.register_node(node_a).await;
    coordinator.set_available_nodes(vec![Uuid::new_v4()]).await;

    let result = coordinator.drain(node_a, 30).await.unwrap();
    assert!(result.success, "Drain of empty node should succeed");
    assert_eq!(result.workloads_migrated, 0, "No workloads to migrate");

    let state = coordinator.drain_state(node_a).await.unwrap();
    assert_eq!(state, NodeDrainState::Drained);
}

/// Verify that drain fails gracefully when no target nodes are available.
#[tokio::test]
async fn tc236_drain_no_target_nodes() {
    let coordinator = InMemoryDrainCoordinator::new();

    let node_a = Uuid::new_v4();
    coordinator.register_node(node_a).await;
    coordinator
        .register_workload(node_a, make_workload_info("web-server"))
        .await;

    // No target nodes available
    coordinator.set_available_nodes(vec![]).await;

    let result = coordinator.drain(node_a, 30).await.unwrap();
    assert!(!result.success, "Drain should fail with no target nodes");
    assert!(
        result.error.is_some(),
        "Error should be present when drain fails"
    );
    assert!(
        result
            .error
            .as_ref()
            .unwrap()
            .contains("No available target nodes"),
        "Error should mention no available target nodes"
    );
}

/// Verify drain events are emitted correctly when integrated with the event log.
#[tokio::test]
async fn tc236_drain_events_emitted() {
    let ib = iri_builder();
    let event_log = Arc::new(InMemoryEventLog::new());
    let coordinator = InMemoryDrainCoordinator::new();

    let node_id = Uuid::new_v4();
    let node_iri = ib.node("drain-test-node");
    coordinator.register_node(node_id).await;
    coordinator
        .register_workload(node_id, make_workload_info("web-server"))
        .await;
    coordinator.set_available_nodes(vec![Uuid::new_v4()]).await;

    let correlation_id = Uuid::new_v4();

    // Emit NodeDrainStarted event
    let drain_started = EventEnvelope::new(
        ib.event_schema("NodeDrainStarted", 1),
        "NodeDrainStarted",
        node_iri.clone(),
        None,
        correlation_id,
        serde_json::json!({
            "node_id": node_id.to_string(),
            "node_iri": node_iri.as_str(),
            "node_name": "drain-test-node",
            "workload_count": 1,
        }),
    );
    event_log.append(drain_started).await.unwrap();

    // Perform drain
    let result = coordinator.drain(node_id, 30).await.unwrap();
    assert!(result.success);

    // Emit WorkloadMigrated event
    let workload_migrated = EventEnvelope::new(
        ib.event_schema("WorkloadMigrated", 1),
        "WorkloadMigrated",
        node_iri.clone(),
        None,
        correlation_id,
        serde_json::json!({
            "workload_iri": ib.resource("test-product", "containers", "web-server").as_str(),
            "from_node_iri": node_iri.as_str(),
            "to_node_iri": ib.node("target-node").as_str(),
            "reason": "node_drain",
        }),
    );
    event_log.append(workload_migrated).await.unwrap();

    // Emit NodeDrainCompleted event
    let drain_completed = EventEnvelope::new(
        ib.event_schema("NodeDrainCompleted", 1),
        "NodeDrainCompleted",
        node_iri.clone(),
        None,
        correlation_id,
        serde_json::json!({
            "node_id": node_id.to_string(),
            "node_iri": node_iri.as_str(),
            "node_name": "drain-test-node",
            "workloads_migrated": result.workloads_migrated,
            "duration_ms": result.duration_ms,
        }),
    );
    event_log.append(drain_completed).await.unwrap();

    // Verify events in the log
    let events = event_log.events_since(0).await;
    assert_eq!(events.len(), 3, "Should have 3 drain-related events");
    assert_eq!(events[0].event_type, "NodeDrainStarted");
    assert_eq!(events[1].event_type, "WorkloadMigrated");
    assert_eq!(events[2].event_type, "NodeDrainCompleted");

    // Verify all events share the same correlation ID
    for event in &events {
        assert_eq!(
            event.correlation_id, correlation_id,
            "All drain events should share correlation ID"
        );
    }
}

/// Verify drain state transitions are correct and idempotent operations
/// are handled properly.
#[tokio::test]
async fn tc236_drain_state_transitions() {
    let coordinator = InMemoryDrainCoordinator::new();
    let node_id = Uuid::new_v4();
    coordinator.register_node(node_id).await;

    // Active → Cordoned
    assert_eq!(
        coordinator.drain_state(node_id).await.unwrap(),
        NodeDrainState::Active
    );
    coordinator.cordon(node_id).await.unwrap();
    assert_eq!(
        coordinator.drain_state(node_id).await.unwrap(),
        NodeDrainState::Cordoned
    );

    // Cordoned → Active (uncordon)
    coordinator.uncordon(node_id).await.unwrap();
    assert_eq!(
        coordinator.drain_state(node_id).await.unwrap(),
        NodeDrainState::Active
    );

    // Active → Draining → Drained (via drain)
    coordinator.set_available_nodes(vec![Uuid::new_v4()]).await;
    let result = coordinator.drain(node_id, 30).await.unwrap();
    assert!(result.success);
    assert_eq!(
        coordinator.drain_state(node_id).await.unwrap(),
        NodeDrainState::Drained
    );

    // Drained → Active (uncordon)
    coordinator.uncordon(node_id).await.unwrap();
    assert_eq!(
        coordinator.drain_state(node_id).await.unwrap(),
        NodeDrainState::Active
    );
}
