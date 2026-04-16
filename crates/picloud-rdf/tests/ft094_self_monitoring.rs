/// FT-094 Integration Tests — Platform self-monitoring via its own RDF graph
///
/// Covers:
///   TC-290: Platform self-monitoring graph contains node health and workload state
///   TC-347: Self-monitoring exit — platform graph contains health data

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::StateProjector;
use picloud_rdf::OxigraphProjector;
use uuid::Uuid;

const PICLOUD_NS: &str = "https://picloud.local/ontology#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

fn iri_builder() -> IriBuilder {
    IriBuilder::new(ClusterDomain::default())
}

/// Extract the "value" field from a SPARQL binding term (uri or literal).
fn val(binding: &serde_json::Value, key: &str) -> String {
    binding
        .get(key)
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn make_event(
    event_type: &str,
    product: Option<&str>,
    payload: serde_json::Value,
) -> EventEnvelope {
    let ib = iri_builder();
    EventEnvelope::new(
        ib.event_schema(event_type, 1),
        event_type,
        ResourceIri::new("https://picloud.local/test").unwrap(),
        product.map(|s| s.to_string()),
        Uuid::new_v4(),
        payload,
    )
}

fn make_node_joined(node_name: &str, node_id: Uuid) -> EventEnvelope {
    let ib = iri_builder();
    let node_iri = ib.node(node_name);
    make_event(
        "NodeJoined",
        None,
        serde_json::json!({
            "node_id": node_id.to_string(),
            "node_name": node_name,
            "node_iri": node_iri.as_str(),
            "address": "192.168.1.101",
        }),
    )
}

fn make_self_monitoring_check(
    node_name: &str,
    overall_status: &str,
    checks: serde_json::Value,
) -> EventEnvelope {
    let ib = iri_builder();
    let node_iri = ib.node(node_name);
    make_event(
        "SelfMonitoringCheckCompleted",
        None,
        serde_json::json!({
            "node_iri": node_iri.as_str(),
            "overall_status": overall_status,
            "checks": checks,
        }),
    )
}

fn make_resource_declared(
    product: &str,
    resource_type: &str,
    name: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let resource_iri = ib.resource(product, &resource_type.to_lowercase().replace(' ', "-"), name);
    make_event(
        "ResourceDeclared",
        Some(product),
        serde_json::json!({
            "resource_iri": resource_iri.as_str(),
            "resource_type": resource_type,
            "product": product,
            "name": name,
        }),
    )
}

fn make_resource_ready(product: &str, resource_type: &str, name: &str) -> EventEnvelope {
    let ib = iri_builder();
    let resource_iri = ib.resource(product, &resource_type.to_lowercase().replace(' ', "-"), name);
    make_event(
        "ResourceReady",
        Some(product),
        serde_json::json!({
            "resource_iri": resource_iri.as_str(),
        }),
    )
}

fn make_resource_failed(product: &str, resource_type: &str, name: &str, reason: &str) -> EventEnvelope {
    let ib = iri_builder();
    let resource_iri = ib.resource(product, &resource_type.to_lowercase().replace(' ', "-"), name);
    make_event(
        "ResourceFailed",
        Some(product),
        serde_json::json!({
            "resource_iri": resource_iri.as_str(),
            "reason": reason,
        }),
    )
}

fn make_workload_migrated(
    product: &str,
    workload_name: &str,
    to_node_name: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let workload_iri = ib.resource(product, "container", workload_name);
    let to_node_iri = ib.node(to_node_name);
    make_event(
        "WorkloadMigrated",
        Some(product),
        serde_json::json!({
            "workload_iri": workload_iri.as_str(),
            "to_node_iri": to_node_iri.as_str(),
        }),
    )
}

// ============================================================================
// TC-290 — Platform self-monitoring graph contains node health and workload state
// ============================================================================
/// Scenario: project self-monitoring check events for multiple nodes and
/// workload resource events, then verify via SPARQL that:
/// 1. Each node has a selfMonitoringStatus
/// 2. Individual health checks are linked via hasHealthCheck
/// 3. Each check has checkName, checkStatus, checkMessage
/// 4. The selfMonitoringCheckedAt timestamp is present
/// 5. Workload resources have status and scheduledOn triples
/// 6. Cross-cutting queries combining node health + workload state work
#[tokio::test]
async fn tc290_platform_self_monitoring_graph_contains_node_health_and_workload_state() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    let node1_id = Uuid::new_v4();
    let node2_id = Uuid::new_v4();

    // --- Step 1: Set up cluster with two nodes ---
    let events = vec![
        make_node_joined("pi-node-01", node1_id),
        make_node_joined("pi-node-02", node2_id),
        // Deploy a product with workloads
        make_resource_declared("photo-app", "Container", "api-server"),
        make_resource_ready("photo-app", "Container", "api-server"),
        make_resource_declared("photo-app", "Container", "worker"),
        make_resource_failed("photo-app", "Container", "worker", "OOM killed"),
        // Schedule workloads on nodes
        make_workload_migrated("photo-app", "api-server", "pi-node-01"),
        make_workload_migrated("photo-app", "worker", "pi-node-02"),
        // Self-monitoring for node-01: healthy
        make_self_monitoring_check("pi-node-01", "healthy", serde_json::json!([
            {"check_name": "raft_health", "status": "healthy", "message": "Raft consensus operating normally"},
            {"check_name": "disk_usage", "status": "healthy", "message": "Disk usage at 42%"},
            {"check_name": "workload_state", "status": "healthy", "message": "All workloads running"},
        ])),
        // Self-monitoring for node-02: degraded (workload failed)
        make_self_monitoring_check("pi-node-02", "degraded", serde_json::json!([
            {"check_name": "raft_health", "status": "healthy", "message": "Raft consensus operating normally"},
            {"check_name": "disk_usage", "status": "degraded", "message": "Disk usage at 89%"},
            {"check_name": "workload_state", "status": "unhealthy", "message": "1 workload failed"},
        ])),
    ];

    for event in &events {
        projector.project(event).await.unwrap();
    }

    // --- Step 2: Verify node health statuses via ASK ---
    let node1_iri = ib.node("pi-node-01");
    let node2_iri = ib.node("pi-node-02");

    let ask = format!(
        "ASK {{ <{}> <{PICLOUD_NS}selfMonitoringStatus> \"healthy\" }}",
        node1_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "pi-node-01 should be healthy");

    let ask = format!(
        "ASK {{ <{}> <{PICLOUD_NS}selfMonitoringStatus> \"degraded\" }}",
        node2_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "pi-node-02 should be degraded");

    // Verify both nodes appear in a SELECT for selfMonitoringStatus
    let health_query = format!(
        "SELECT ?node ?status WHERE {{ \
            ?node <{RDF_TYPE}> <{PICLOUD_NS}Node> ; \
                  <{PICLOUD_NS}selfMonitoringStatus> ?status \
        }} ORDER BY ?node"
    );
    let result = projector.query(&health_query).await.unwrap();
    assert_eq!(result.bindings.len(), 2, "expected 2 nodes with selfMonitoringStatus");

    // --- Step 3: Verify individual health checks are projected ---
    let checks_query = format!(
        "SELECT ?node ?checkName ?checkStatus ?checkMessage WHERE {{ \
            ?node <{PICLOUD_NS}hasHealthCheck> ?check . \
            ?check <{RDF_TYPE}> <{PICLOUD_NS}HealthCheck> ; \
                   <{PICLOUD_NS}checkName> ?checkName ; \
                   <{PICLOUD_NS}checkStatus> ?checkStatus ; \
                   <{PICLOUD_NS}checkMessage> ?checkMessage \
        }} ORDER BY ?node ?checkName"
    );
    let result = projector.query(&checks_query).await.unwrap();
    // 3 checks per node * 2 nodes = 6 total
    assert_eq!(result.bindings.len(), 6, "expected 6 individual health checks (3 per node)");

    // Verify node-01's raft_health check
    let node1_raft = result.bindings.iter().find(|b| {
        val(b, "node") == node1_iri.as_str() && val(b, "checkName") == "raft_health"
    });
    assert!(node1_raft.is_some(), "node-01 should have a raft_health check");
    assert_eq!(val(node1_raft.unwrap(), "checkStatus"), "healthy");

    // Verify node-02's disk_usage check is degraded
    let node2_disk = result.bindings.iter().find(|b| {
        val(b, "node") == node2_iri.as_str() && val(b, "checkName") == "disk_usage"
    });
    assert!(node2_disk.is_some(), "node-02 should have a disk_usage check");
    assert_eq!(val(node2_disk.unwrap(), "checkStatus"), "degraded");

    // Verify node-02's workload_state check is unhealthy
    let node2_workload = result.bindings.iter().find(|b| {
        val(b, "node") == node2_iri.as_str() && val(b, "checkName") == "workload_state"
    });
    assert!(node2_workload.is_some(), "node-02 should have a workload_state check");
    assert_eq!(val(node2_workload.unwrap(), "checkStatus"), "unhealthy");
    assert_eq!(val(node2_workload.unwrap(), "checkMessage"), "1 workload failed");

    // --- Step 4: Verify selfMonitoringCheckedAt timestamp is present ---
    let timestamp_query = format!(
        "ASK {{ ?node <{PICLOUD_NS}selfMonitoringCheckedAt> ?ts }}"
    );
    let result = projector.query(&timestamp_query).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "at least one node should have a selfMonitoringCheckedAt timestamp"
    );

    // --- Step 5: Verify workload state (status + scheduledOn) ---
    let workload_query = format!(
        "SELECT ?workload ?status ?node WHERE {{ \
            ?workload <{RDF_TYPE}> <{PICLOUD_NS}Resource> ; \
                      <{PICLOUD_NS}resourceType> \"Container\" ; \
                      <{PICLOUD_NS}status> ?status ; \
                      <{PICLOUD_NS}scheduledOn> ?node \
        }} ORDER BY ?workload"
    );
    let result = projector.query(&workload_query).await.unwrap();
    assert_eq!(result.bindings.len(), 2, "expected 2 workloads with status + scheduledOn");

    // api-server should be Ready on node-01
    let api_server_iri = ib.resource("photo-app", "container", "api-server");
    let api_binding = result.bindings.iter().find(|b| {
        val(b, "workload") == api_server_iri.as_str()
    });
    assert!(api_binding.is_some(), "api-server workload should be in results");
    assert_eq!(
        val(api_binding.unwrap(), "status"),
        format!("{PICLOUD_NS}Ready"),
        "api-server should have Ready status"
    );
    assert_eq!(
        val(api_binding.unwrap(), "node"),
        node1_iri.as_str(),
        "api-server should be scheduled on pi-node-01"
    );

    // worker should be Failed on node-02
    let worker_iri = ib.resource("photo-app", "container", "worker");
    let worker_binding = result.bindings.iter().find(|b| {
        val(b, "workload") == worker_iri.as_str()
    });
    assert!(worker_binding.is_some(), "worker workload should be in results");
    assert_eq!(
        val(worker_binding.unwrap(), "status"),
        format!("{PICLOUD_NS}Failed"),
        "worker should have Failed status"
    );
    assert_eq!(
        val(worker_binding.unwrap(), "node"),
        node2_iri.as_str(),
        "worker should be scheduled on pi-node-02"
    );

    // --- Step 6: Cross-cutting query — degraded nodes with their checks ---
    let degraded_query = format!(
        "SELECT ?node ?checkName ?checkStatus WHERE {{ \
            ?node <{PICLOUD_NS}selfMonitoringStatus> \"degraded\" ; \
                  <{PICLOUD_NS}hasHealthCheck> ?check . \
            ?check <{PICLOUD_NS}checkName> ?checkName ; \
                   <{PICLOUD_NS}checkStatus> ?checkStatus \
        }} ORDER BY ?checkName"
    );
    let result = projector.query(&degraded_query).await.unwrap();
    assert_eq!(result.bindings.len(), 3, "degraded node-02 should have 3 checks");
    assert!(
        result.bindings.iter().all(|b| val(b, "node") == node2_iri.as_str()),
        "all results should be from the degraded node"
    );
}

// ============================================================================
// TC-347 — Self-monitoring exit — platform graph contains health data
// ============================================================================
/// Exit criteria: project a comprehensive set of self-monitoring events into
/// the RDF graph and verify the platform graph contains complete health data.
///
/// This test validates the end-to-end self-monitoring flow:
/// 1. Nodes join and get health monitoring
/// 2. Health data updates (upsert pattern) replace old values
/// 3. SPARQL can answer "which nodes are unhealthy?" and "what failed?"
/// 4. Workload scheduling state is queryable alongside health data
/// 5. The graph contains everything needed for a platform health dashboard
#[tokio::test]
async fn tc347_self_monitoring_exit_platform_graph_contains_health_data() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    let node1_id = Uuid::new_v4();
    let node2_id = Uuid::new_v4();
    let node3_id = Uuid::new_v4();

    // --- Build a 3-node cluster with workloads and monitoring ---
    let setup_events = vec![
        make_node_joined("pi-node-01", node1_id),
        make_node_joined("pi-node-02", node2_id),
        make_node_joined("pi-node-03", node3_id),
        // Workloads across the cluster
        make_resource_declared("photo-app", "Container", "api-server"),
        make_resource_ready("photo-app", "Container", "api-server"),
        make_workload_migrated("photo-app", "api-server", "pi-node-01"),
        make_resource_declared("chat-app", "Container", "ws-server"),
        make_resource_ready("chat-app", "Container", "ws-server"),
        make_workload_migrated("chat-app", "ws-server", "pi-node-02"),
    ];

    for event in &setup_events {
        projector.project(event).await.unwrap();
    }

    // --- First round of health checks: all healthy ---
    let round1_events = vec![
        make_self_monitoring_check("pi-node-01", "healthy", serde_json::json!([
            {"check_name": "raft_health", "status": "healthy", "message": "Raft OK"},
            {"check_name": "projection_lag", "status": "healthy", "message": "Lag: 0 events"},
        ])),
        make_self_monitoring_check("pi-node-02", "healthy", serde_json::json!([
            {"check_name": "raft_health", "status": "healthy", "message": "Raft OK"},
            {"check_name": "projection_lag", "status": "healthy", "message": "Lag: 0 events"},
        ])),
        make_self_monitoring_check("pi-node-03", "healthy", serde_json::json!([
            {"check_name": "raft_health", "status": "healthy", "message": "Raft OK"},
            {"check_name": "projection_lag", "status": "healthy", "message": "Lag: 0 events"},
        ])),
    ];

    for event in &round1_events {
        projector.project(event).await.unwrap();
    }

    // Verify all 3 nodes are healthy via ASK
    let ask = format!(
        "ASK {{ \
            ?n1 <{PICLOUD_NS}selfMonitoringStatus> \"healthy\" . \
            ?n2 <{PICLOUD_NS}selfMonitoringStatus> \"healthy\" . \
            ?n3 <{PICLOUD_NS}selfMonitoringStatus> \"healthy\" . \
            FILTER(?n1 != ?n2 && ?n1 != ?n3 && ?n2 != ?n3) \
        }}"
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "all 3 nodes should be healthy");

    // --- Second round: node-03 goes unhealthy (upsert replaces old data) ---
    let round2_events = vec![
        make_self_monitoring_check("pi-node-03", "unhealthy", serde_json::json!([
            {"check_name": "raft_health", "status": "unhealthy", "message": "Lost quorum"},
            {"check_name": "projection_lag", "status": "degraded", "message": "Lag: 500 events"},
        ])),
    ];

    for event in &round2_events {
        projector.project(event).await.unwrap();
    }

    // Verify upsert: node-03 should now be unhealthy
    let node3_iri = ib.node("pi-node-03");
    let ask = format!(
        "ASK {{ <{}> <{PICLOUD_NS}selfMonitoringStatus> \"unhealthy\" }}",
        node3_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "node-03 should be unhealthy");

    // Verify node-01 and node-02 are still healthy
    let node1_iri = ib.node("pi-node-01");
    let node2_iri = ib.node("pi-node-02");
    let ask = format!(
        "ASK {{ <{}> <{PICLOUD_NS}selfMonitoringStatus> \"healthy\" }}",
        node1_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "node-01 should still be healthy");

    let ask = format!(
        "ASK {{ <{}> <{PICLOUD_NS}selfMonitoringStatus> \"healthy\" }}",
        node2_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "node-02 should still be healthy");

    // Verify the old checks for node-03 were replaced (not accumulated)
    let node3_checks_query = format!(
        "SELECT ?checkName ?checkStatus ?checkMessage WHERE {{ \
            <{node3}> <{PICLOUD_NS}hasHealthCheck> ?check . \
            ?check <{PICLOUD_NS}checkName> ?checkName ; \
                   <{PICLOUD_NS}checkStatus> ?checkStatus ; \
                   <{PICLOUD_NS}checkMessage> ?checkMessage \
        }} ORDER BY ?checkName",
        node3 = node3_iri.as_str()
    );
    let result = projector.query(&node3_checks_query).await.unwrap();
    assert_eq!(
        result.bindings.len(), 2,
        "node-03 should have exactly 2 checks (replaced, not accumulated)"
    );

    // Verify the raft_health check is now unhealthy
    let raft_check = result.bindings.iter().find(|b| val(b, "checkName") == "raft_health");
    assert!(raft_check.is_some());
    assert_eq!(val(raft_check.unwrap(), "checkStatus"), "unhealthy");
    assert_eq!(val(raft_check.unwrap(), "checkMessage"), "Lost quorum");

    // Verify projection_lag is now degraded
    let lag_check = result.bindings.iter().find(|b| val(b, "checkName") == "projection_lag");
    assert!(lag_check.is_some());
    assert_eq!(val(lag_check.unwrap(), "checkStatus"), "degraded");

    // --- Verify selfMonitoringCheckedAt timestamps exist for all monitored nodes ---
    let ts_query = format!(
        "SELECT ?node WHERE {{ ?node <{PICLOUD_NS}selfMonitoringCheckedAt> ?ts }}"
    );
    let result = projector.query(&ts_query).await.unwrap();
    assert_eq!(
        result.bindings.len(), 3,
        "all 3 nodes should have a selfMonitoringCheckedAt timestamp"
    );

    // --- Dashboard query: node health + workload state combined ---
    let dashboard_query = format!(
        "SELECT ?node ?nodeName ?healthStatus ?workload ?workloadStatus WHERE {{ \
            ?node <{RDF_TYPE}> <{PICLOUD_NS}Node> ; \
                  <{PICLOUD_NS}nodeName> ?nodeName . \
            OPTIONAL {{ ?node <{PICLOUD_NS}selfMonitoringStatus> ?healthStatus }} \
            OPTIONAL {{ \
                ?workload <{PICLOUD_NS}scheduledOn> ?node ; \
                          <{PICLOUD_NS}status> ?workloadStatus \
            }} \
        }} ORDER BY ?nodeName"
    );
    let result = projector.query(&dashboard_query).await.unwrap();
    // Should have rows for all nodes (some with workloads, some without)
    assert!(
        result.bindings.len() >= 3,
        "dashboard query should return at least one row per node, got: {}",
        result.bindings.len()
    );

    // Verify node-01 row includes its workload and health
    let node1_row = result.bindings.iter().find(|b| {
        val(b, "node") == node1_iri.as_str() && !val(b, "workload").is_empty()
    });
    assert!(node1_row.is_some(), "node-01 should appear with its workload");
    assert_eq!(val(node1_row.unwrap(), "healthStatus"), "healthy");
    assert_eq!(
        val(node1_row.unwrap(), "workloadStatus"),
        format!("{PICLOUD_NS}Ready")
    );

    // --- Find unhealthy nodes and their failing checks in one query ---
    let unhealthy_details_query = format!(
        "SELECT ?node ?nodeName ?checkName ?checkStatus ?checkMessage WHERE {{ \
            ?node <{RDF_TYPE}> <{PICLOUD_NS}Node> ; \
                  <{PICLOUD_NS}nodeName> ?nodeName ; \
                  <{PICLOUD_NS}selfMonitoringStatus> ?status ; \
                  <{PICLOUD_NS}hasHealthCheck> ?check . \
            ?check <{PICLOUD_NS}checkName> ?checkName ; \
                   <{PICLOUD_NS}checkStatus> ?checkStatus ; \
                   <{PICLOUD_NS}checkMessage> ?checkMessage . \
            FILTER(?status != \"healthy\") \
        }} ORDER BY ?nodeName ?checkName"
    );
    let result = projector.query(&unhealthy_details_query).await.unwrap();
    assert_eq!(
        result.bindings.len(), 2,
        "unhealthy node-03 should have 2 checks in the result"
    );
    assert!(
        result.bindings.iter().all(|b| val(b, "nodeName") == "pi-node-03"),
        "all results should be from pi-node-03"
    );

    // --- Final verification: all health check types are present cluster-wide ---
    let all_check_types_query = format!(
        "SELECT DISTINCT ?checkName WHERE {{ \
            ?check <{RDF_TYPE}> <{PICLOUD_NS}HealthCheck> ; \
                   <{PICLOUD_NS}checkName> ?checkName \
        }} ORDER BY ?checkName"
    );
    let result = projector.query(&all_check_types_query).await.unwrap();
    let check_names: Vec<String> = result.bindings.iter()
        .map(|b| val(b, "checkName"))
        .collect();
    assert!(check_names.contains(&"projection_lag".to_string()), "should have projection_lag checks");
    assert!(check_names.contains(&"raft_health".to_string()), "should have raft_health checks");
}
