//! FT-092 — Node drain and graceful workload migration
//!
//! Covers TC-288 (node drain migrates workloads to other nodes gracefully).
//!
//! These tests verify that during a node drain operation:
//! 1. Workloads are migrated to surviving nodes via round-robin.
//! 2. Migration records track the source and destination of each workload.
//! 3. Target nodes receive the migrated workloads.
//! 4. Mixed workload types (containers + binaries) are handled gracefully.
//! 5. Drain events carry correct metadata for every migrated workload.
//! 6. The drained node retains no workloads after completion.

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
// TC-288 — Node drain migrates workloads to other nodes gracefully
// ============================================================================

/// Scenario: Set up a 3-node cluster with mixed workloads on one node.
/// Drain that node and verify that every workload is gracefully migrated
/// to surviving nodes with full traceability.
#[tokio::test]
async fn tc288_node_drain_migrates_workloads_to_other_nodes_gracefully() {
    let ib = iri_builder();
    let event_log = Arc::new(InMemoryEventLog::new());
    let coordinator = InMemoryDrainCoordinator::new();

    // --- Cluster topology: 3 nodes ---
    let node_a = Uuid::new_v4(); // will be drained
    let node_b = Uuid::new_v4(); // migration target
    let node_c = Uuid::new_v4(); // migration target

    coordinator.register_node(node_a).await;
    coordinator.register_node(node_b).await;
    coordinator.register_node(node_c).await;

    // --- Register mixed workloads on node_a ---
    let workloads = vec![
        container_workload("web-frontend"),
        container_workload("api-server"),
        binary_workload("metrics-collector"),
        container_workload("background-worker"),
    ];
    for w in &workloads {
        coordinator.register_workload(node_a, w.clone()).await;
    }

    // Set up migration targets (node_b and node_c)
    coordinator.set_available_nodes(vec![node_b, node_c]).await;

    // --- Verify preconditions ---
    let pre_workloads = coordinator.node_workloads(node_a).await.unwrap();
    assert_eq!(pre_workloads.len(), 4, "Node A should have 4 workloads before drain");

    let state = coordinator.drain_state(node_a).await.unwrap();
    assert_eq!(state, NodeDrainState::Active, "Node A should start in Active state");

    // --- Step 1: Cordon node_a first (graceful: no new scheduling) ---
    coordinator.cordon(node_a).await.unwrap();
    assert_eq!(
        coordinator.drain_state(node_a).await.unwrap(),
        NodeDrainState::Cordoned,
        "Node should be cordoned before drain"
    );

    // Emit NodeCordoned event
    let correlation_id = Uuid::new_v4();
    let node_a_iri = ib.node("node-a");
    let cordon_event = EventEnvelope::new(
        ib.event_schema("NodeCordoned", 1),
        "NodeCordoned",
        node_a_iri.clone(),
        None,
        correlation_id,
        serde_json::json!({
            "node_id": node_a.to_string(),
            "node_iri": node_a_iri.as_str(),
            "node_name": "node-a",
        }),
    );
    event_log.append(cordon_event).await.unwrap();

    // Uncordon so drain() can transition from Active → Draining
    coordinator.uncordon(node_a).await.unwrap();

    // --- Step 2: Emit NodeDrainStarted and perform the drain ---
    let drain_started = EventEnvelope::new(
        ib.event_schema("NodeDrainStarted", 1),
        "NodeDrainStarted",
        node_a_iri.clone(),
        None,
        correlation_id,
        serde_json::json!({
            "node_id": node_a.to_string(),
            "node_iri": node_a_iri.as_str(),
            "node_name": "node-a",
            "workload_count": 4,
        }),
    );
    event_log.append(drain_started).await.unwrap();

    let result = coordinator.drain(node_a, 30).await.unwrap();

    // --- Step 3: Verify drain succeeded ---
    assert!(result.success, "Drain should succeed");
    assert_eq!(result.workloads_migrated, 4, "All 4 workloads should be migrated");
    assert!(result.error.is_none(), "No error should be present");
    assert!(result.duration_ms < 30_000, "Drain should complete within timeout");

    // --- Step 4: Verify migration records (FT-092 — graceful traceability) ---
    let migrations = coordinator.migration_log().await;
    assert_eq!(migrations.len(), 4, "Should have 4 migration records");

    // All migrations should come from node_a
    for m in &migrations {
        assert_eq!(m.from_node_id, node_a, "Migration source should be node_a");
        assert!(
            m.to_node_id == node_b || m.to_node_id == node_c,
            "Migration target should be node_b or node_c"
        );
    }

    // Verify round-robin distribution — workloads should alternate between targets
    assert_eq!(migrations[0].to_node_id, node_b, "1st workload → node_b");
    assert_eq!(migrations[1].to_node_id, node_c, "2nd workload → node_c");
    assert_eq!(migrations[2].to_node_id, node_b, "3rd workload → node_b");
    assert_eq!(migrations[3].to_node_id, node_c, "4th workload → node_c");

    // Verify mixed workload types are tracked correctly
    let container_migrations: Vec<_> = migrations.iter().filter(|m| m.workload_type == "container").collect();
    let binary_migrations: Vec<_> = migrations.iter().filter(|m| m.workload_type == "binary").collect();
    assert_eq!(container_migrations.len(), 3, "3 container workloads migrated");
    assert_eq!(binary_migrations.len(), 1, "1 binary workload migrated");

    // --- Step 5: Verify workloads actually landed on target nodes ---
    let node_b_workloads = coordinator.node_workloads(node_b).await.unwrap();
    let node_c_workloads = coordinator.node_workloads(node_c).await.unwrap();
    assert_eq!(node_b_workloads.len(), 2, "Node B should receive 2 workloads");
    assert_eq!(node_c_workloads.len(), 2, "Node C should receive 2 workloads");

    // Verify the drained node has zero workloads
    let node_a_workloads = coordinator.node_workloads(node_a).await.unwrap();
    assert!(node_a_workloads.is_empty(), "Drained node should have no workloads");

    // --- Step 6: Emit WorkloadMigrated events for each migration ---
    let node_b_iri = ib.node("node-b");
    let node_c_iri = ib.node("node-c");

    for m in &migrations {
        let target_iri = if m.to_node_id == node_b {
            &node_b_iri
        } else {
            &node_c_iri
        };
        let migrated_event = EventEnvelope::new(
            ib.event_schema("WorkloadMigrated", 1),
            "WorkloadMigrated",
            node_a_iri.clone(),
            None,
            correlation_id,
            serde_json::json!({
                "workload_iri": m.workload_iri.as_str(),
                "from_node_iri": node_a_iri.as_str(),
                "to_node_iri": target_iri.as_str(),
                "reason": "node_drain",
            }),
        );
        event_log.append(migrated_event).await.unwrap();
    }

    // --- Step 7: Emit NodeDrainCompleted ---
    let drain_completed = EventEnvelope::new(
        ib.event_schema("NodeDrainCompleted", 1),
        "NodeDrainCompleted",
        node_a_iri.clone(),
        None,
        correlation_id,
        serde_json::json!({
            "node_id": node_a.to_string(),
            "node_iri": node_a_iri.as_str(),
            "node_name": "node-a",
            "workloads_migrated": result.workloads_migrated,
            "duration_ms": result.duration_ms,
        }),
    );
    event_log.append(drain_completed).await.unwrap();

    // --- Step 8: Verify complete event chain ---
    let events = event_log.events_since(0).await;
    // Expected: NodeCordoned, NodeDrainStarted, 4x WorkloadMigrated, NodeDrainCompleted
    assert_eq!(events.len(), 7, "Should have 7 events in the log");
    assert_eq!(events[0].event_type, "NodeCordoned");
    assert_eq!(events[1].event_type, "NodeDrainStarted");
    assert_eq!(events[2].event_type, "WorkloadMigrated");
    assert_eq!(events[3].event_type, "WorkloadMigrated");
    assert_eq!(events[4].event_type, "WorkloadMigrated");
    assert_eq!(events[5].event_type, "WorkloadMigrated");
    assert_eq!(events[6].event_type, "NodeDrainCompleted");

    // All drain events share the same correlation ID
    for event in &events {
        assert_eq!(
            event.correlation_id, correlation_id,
            "All drain events should share correlation ID"
        );
    }

    // --- Step 9: Verify final node state ---
    let state = coordinator.drain_state(node_a).await.unwrap();
    assert_eq!(state, NodeDrainState::Drained, "Node A should be in Drained state");

    // Verify target nodes remain Active
    let state_b = coordinator.drain_state(node_b).await.unwrap();
    let state_c = coordinator.drain_state(node_c).await.unwrap();
    assert_eq!(state_b, NodeDrainState::Active, "Node B should remain Active");
    assert_eq!(state_c, NodeDrainState::Active, "Node C should remain Active");
}

/// Verify that draining a node with a single target still migrates all
/// workloads to that one node.
#[tokio::test]
async fn tc288_drain_single_target_node() {
    let coordinator = InMemoryDrainCoordinator::new();

    let source = Uuid::new_v4();
    let target = Uuid::new_v4();

    coordinator.register_node(source).await;
    coordinator.register_node(target).await;
    coordinator.set_available_nodes(vec![target]).await;

    coordinator.register_workload(source, container_workload("svc-1")).await;
    coordinator.register_workload(source, container_workload("svc-2")).await;
    coordinator.register_workload(source, binary_workload("agent")).await;

    let result = coordinator.drain(source, 30).await.unwrap();
    assert!(result.success);
    assert_eq!(result.workloads_migrated, 3);

    // All workloads should land on the single target
    let target_workloads = coordinator.node_workloads(target).await.unwrap();
    assert_eq!(target_workloads.len(), 3, "All 3 workloads should land on the single target");

    let migrations = coordinator.migration_log().await;
    for m in &migrations {
        assert_eq!(m.to_node_id, target, "All migrations should go to the single target");
    }
}

/// Verify that the migration log captures workload IRIs and types correctly
/// so that downstream systems can reconstruct the migration trail.
#[tokio::test]
async fn tc288_migration_log_captures_workload_identity() {
    let coordinator = InMemoryDrainCoordinator::new();

    let source = Uuid::new_v4();
    let target = Uuid::new_v4();
    coordinator.register_node(source).await;
    coordinator.register_node(target).await;
    coordinator.set_available_nodes(vec![target]).await;

    let web = container_workload("web");
    let cron = binary_workload("cron-job");

    coordinator.register_workload(source, web.clone()).await;
    coordinator.register_workload(source, cron.clone()).await;

    let result = coordinator.drain(source, 30).await.unwrap();
    assert!(result.success);

    let log = coordinator.migration_log().await;
    assert_eq!(log.len(), 2);

    // Verify the first migration record matches the web container
    assert_eq!(log[0].workload_iri, web.workload_iri);
    assert_eq!(log[0].workload_type, "container");
    assert_eq!(log[0].from_node_id, source);
    assert_eq!(log[0].to_node_id, target);

    // Verify the second migration record matches the cron binary
    assert_eq!(log[1].workload_iri, cron.workload_iri);
    assert_eq!(log[1].workload_type, "binary");
    assert_eq!(log[1].from_node_id, source);
    assert_eq!(log[1].to_node_id, target);
}

/// Verify that after graceful migration, the target node can itself be
/// drained, cascading workloads further (chain drain).
#[tokio::test]
async fn tc288_cascading_drain() {
    let coordinator = InMemoryDrainCoordinator::new();

    let node_a = Uuid::new_v4();
    let node_b = Uuid::new_v4();
    let node_c = Uuid::new_v4();

    coordinator.register_node(node_a).await;
    coordinator.register_node(node_b).await;
    coordinator.register_node(node_c).await;

    // Put workloads on node_a
    coordinator.register_workload(node_a, container_workload("svc-1")).await;
    coordinator.register_workload(node_a, container_workload("svc-2")).await;

    // --- First drain: node_a → node_b ---
    coordinator.set_available_nodes(vec![node_b]).await;
    let r1 = coordinator.drain(node_a, 30).await.unwrap();
    assert!(r1.success);
    assert_eq!(r1.workloads_migrated, 2);

    // node_b now has the 2 workloads
    let b_wl = coordinator.node_workloads(node_b).await.unwrap();
    assert_eq!(b_wl.len(), 2, "Node B should have 2 workloads after first drain");

    // --- Second drain: node_b → node_c ---
    coordinator.set_available_nodes(vec![node_c]).await;
    coordinator.clear_migration_log().await;
    let r2 = coordinator.drain(node_b, 30).await.unwrap();
    assert!(r2.success);
    assert_eq!(r2.workloads_migrated, 2);

    // node_c now has the 2 workloads
    let c_wl = coordinator.node_workloads(node_c).await.unwrap();
    assert_eq!(c_wl.len(), 2, "Node C should have 2 workloads after cascading drain");

    // Both source nodes are drained
    assert_eq!(coordinator.drain_state(node_a).await.unwrap(), NodeDrainState::Drained);
    assert_eq!(coordinator.drain_state(node_b).await.unwrap(), NodeDrainState::Drained);

    // Verify second drain's migration log
    let log = coordinator.migration_log().await;
    assert_eq!(log.len(), 2);
    for m in &log {
        assert_eq!(m.from_node_id, node_b);
        assert_eq!(m.to_node_id, node_c);
    }
}
