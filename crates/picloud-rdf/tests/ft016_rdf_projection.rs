/// FT-016 Integration Tests — Oxigraph RDF Projection of Cluster State
///
/// Covers TC-294: Exit criteria verifying that SPARQL queries return
/// projected cluster state after events are projected into Oxigraph.

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

fn make_product_deployed(product_name: &str, version: &str) -> EventEnvelope {
    let ib = iri_builder();
    let product_iri = ib.product(product_name);
    make_event(
        "ProductDeployed",
        Some(product_name),
        serde_json::json!({
            "product_iri": product_iri.as_str(),
            "product_name": product_name,
            "version": version,
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

fn make_identity_created(name: &str) -> EventEnvelope {
    let ib = iri_builder();
    let identity_iri = ib.resource("platform", "identities", name);
    make_event(
        "IdentityCreated",
        None,
        serde_json::json!({
            "identity_iri": identity_iri.as_str(),
            "identity_type": "Human",
            "name": name,
        }),
    )
}

fn make_leader_elected(node_name: &str) -> EventEnvelope {
    let ib = iri_builder();
    let node_iri = ib.node(node_name);
    make_event(
        "LeaderElected",
        None,
        serde_json::json!({
            "node_iri": node_iri.as_str(),
            "node_name": node_name,
            "term": 1,
        }),
    )
}

fn make_metric_recorded(node_name: &str) -> EventEnvelope {
    let ib = iri_builder();
    let node_iri = ib.node(node_name);
    make_event(
        "MetricRecorded",
        None,
        serde_json::json!({
            "node_iri": node_iri.as_str(),
            "metrics": [
                {"name": "cpu_usage_percent", "value": 42.5},
                {"name": "memory_used_mb", "value": 1024.0},
                {"name": "memory_total_mb", "value": 4096.0},
                {"name": "disk_used_gb", "value": 50.0},
                {"name": "disk_total_gb", "value": 200.0},
                {"name": "cpu_temp_celsius", "value": 55.0},
            ],
        }),
    )
}

// ============================================================================
// TC-294 — RDF projection exit — SPARQL query returns projected cluster state
// ============================================================================
/// Exit criteria for FT-016: Project a representative cluster state (nodes,
/// products, containers, volumes, identities, leader election, metrics) into
/// Oxigraph via the event projection pipeline, then verify the full cluster
/// state is queryable via SPARQL SELECT, ASK, CONSTRUCT, and DESCRIBE queries.
///
/// This test validates the complete end-to-end flow:
/// 1. Events are projected into RDF triples
/// 2. Nodes, products, and resources appear with correct types
/// 3. Resource status transitions are reflected
/// 4. Product-scoped resources land in named graphs
/// 5. Cross-cutting queries (all resources, all nodes) work
/// 6. Leader role is assigned to the correct node
/// 7. Metrics are queryable
#[tokio::test]
async fn tc294_rdf_projection_exit_sparql_query_returns_projected() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // --- Step 1: Build up representative cluster state via events ---

    let node1_id = Uuid::new_v4();
    let node2_id = Uuid::new_v4();

    let events = vec![
        // Two nodes join the cluster
        make_node_joined("pi-node-01", node1_id),
        make_node_joined("pi-node-02", node2_id),
        // Elect a leader
        make_leader_elected("pi-node-01"),
        // Deploy two products
        make_product_deployed("photo-app", "1.0.0"),
        make_product_deployed("chat-app", "2.0.0"),
        // photo-app: container + volume
        make_resource_declared("photo-app", "Container", "api-server"),
        make_resource_ready("photo-app", "Container", "api-server"),
        make_resource_declared("photo-app", "Volume", "media-store"),
        make_resource_ready("photo-app", "Volume", "media-store"),
        // chat-app: container
        make_resource_declared("chat-app", "Container", "ws-server"),
        make_resource_ready("chat-app", "Container", "ws-server"),
        // Platform identity
        make_identity_created("alice"),
        // Node metrics
        make_metric_recorded("pi-node-01"),
    ];

    for event in &events {
        projector.project(event).await.unwrap();
    }

    // --- Step 2: Verify nodes are projected ---

    let node_query = format!(
        "SELECT ?node ?name WHERE {{ ?node <{RDF_TYPE}> <{PICLOUD_NS}Node> ; <{PICLOUD_NS}nodeName> ?name }}"
    );
    let result = projector.query(&node_query).await.unwrap();
    assert_eq!(
        result.bindings.len(), 2,
        "Should have exactly 2 nodes projected, got: {:?}", result.bindings
    );

    // Verify specific node IRIs exist
    let node1_iri = ib.node("pi-node-01");
    let ask = format!("ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}Node> }}", node1_iri.as_str());
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "pi-node-01 should exist as Node");

    let node2_iri = ib.node("pi-node-02");
    let ask = format!("ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}Node> }}", node2_iri.as_str());
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "pi-node-02 should exist as Node");

    // --- Step 3: Verify leader election ---

    let ask = format!(
        "ASK {{ <{}> <{PICLOUD_NS}hasRole> <{PICLOUD_NS}Leader> }}",
        node1_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "pi-node-01 should be Leader");

    // Node 2 should NOT be leader
    let ask = format!(
        "ASK {{ <{}> <{PICLOUD_NS}hasRole> <{PICLOUD_NS}Leader> }}",
        node2_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], false, "pi-node-02 should NOT be Leader");

    // --- Step 4: Verify products are projected ---
    // Products are stored as rdf:type picloud:Resource with picloud:resourceType "Product"

    let product_query = format!(
        "SELECT ?p ?version WHERE {{ ?p <{PICLOUD_NS}resourceType> \"Product\" ; <{PICLOUD_NS}version> ?version }}"
    );
    let result = projector.query(&product_query).await.unwrap();
    assert_eq!(
        result.bindings.len(), 2,
        "Should have 2 products, got: {:?}", result.bindings
    );

    // --- Step 5: Verify resources with correct types and statuses ---

    // photo-app container should be Ready
    let container_iri = ib.resource("photo-app", "container", "api-server");
    let ask = format!(
        "ASK {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}Container> ; <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> }}",
        iri = container_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "photo-app/container/api-server should be Ready");

    // photo-app volume should be Ready
    let volume_iri = ib.resource("photo-app", "volume", "media-store");
    let ask = format!(
        "ASK {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}Volume> ; <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> }}",
        iri = volume_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "photo-app/volume/media-store should be Ready");

    // chat-app container should be Ready
    let ws_iri = ib.resource("chat-app", "container", "ws-server");
    let ask = format!(
        "ASK {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}Container> ; <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> }}",
        iri = ws_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "chat-app/container/ws-server should be Ready");

    // --- Step 6: Verify identity ---

    let identity_iri = ib.resource("platform", "identities", "alice");
    let ask = format!(
        "ASK {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}Identity> }}",
        iri = identity_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "Identity alice should exist as Identity type");

    let ask = format!(
        "ASK {{ <{iri}> <{PICLOUD_NS}name> \"alice\" }}",
        iri = identity_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "Identity alice should have name 'alice'");

    // --- Step 7: Verify metrics are queryable ---

    let metrics_query = format!(
        "SELECT ?cpu WHERE {{ <{node}> <{PICLOUD_NS}cpuUsagePercent> ?cpu }}",
        node = node1_iri.as_str()
    );
    let result = projector.query(&metrics_query).await.unwrap();
    assert!(
        !result.bindings.is_empty(),
        "Node metrics should be queryable"
    );

    // --- Step 8: Verify product-scoped named graph isolation ---
    // Named graphs store rdf:type picloud:Resource (not the specific subtype)

    // photo-app's container should appear in photo-app's named graph
    let photo_graph = ib.product_graph("photo-app");
    let graph_query = format!(
        "ASK {{ GRAPH <{graph}> {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}Resource> }} }}",
        graph = photo_graph.as_str(),
        iri = container_iri.as_str()
    );
    let result = projector.query(&graph_query).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "Container should be in photo-app's named graph");

    // chat-app's container should NOT be in photo-app's named graph
    let graph_query = format!(
        "ASK {{ GRAPH <{graph}> {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}Resource> }} }}",
        graph = photo_graph.as_str(),
        iri = ws_iri.as_str()
    );
    let result = projector.query(&graph_query).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "chat-app container should NOT be in photo-app's named graph"
    );

    // --- Step 9: Cross-cutting query — count all resources in default graph ---

    let all_resources_query = format!(
        "SELECT (COUNT(DISTINCT ?s) AS ?count) WHERE {{ ?s <{PICLOUD_NS}status> ?status }}"
    );
    let result = projector.query(&all_resources_query).await.unwrap();
    let count: usize = result.bindings[0]["count"]["value"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    // 2 products + 3 resources (2 photo-app + 1 chat-app) = 5 with status
    assert!(
        count >= 5,
        "Should have at least 5 resources with status, got {count}"
    );

    // --- Step 10: CONSTRUCT query produces valid triples ---
    // Note: Products are rdf:type picloud:Resource (not picloud:Product)
    // Resources with specific types: Node(2), Container(2), Volume(1), Identity(1), Resource(5)

    let construct = format!(
        "CONSTRUCT {{ ?s <{RDF_TYPE}> ?type }} WHERE {{ ?s <{RDF_TYPE}> ?type . FILTER(?type IN (<{PICLOUD_NS}Node>, <{PICLOUD_NS}Resource>, <{PICLOUD_NS}Container>, <{PICLOUD_NS}Volume>, <{PICLOUD_NS}Identity>)) }}"
    );
    let result = projector.query(&construct).await.unwrap();
    // 2 nodes + 5 Resource (2 products + 3 resources) + 2 containers + 1 volume + 1 identity = 11 typed triples
    assert!(
        result.bindings.len() >= 8,
        "CONSTRUCT should return at least 8 typed triples, got {}",
        result.bindings.len()
    );

    // --- Step 11: DESCRIBE returns triples for a specific resource ---

    let describe = format!("DESCRIBE <{}>", container_iri.as_str());
    let result = projector.query(&describe).await.unwrap();
    assert!(
        !result.bindings.is_empty(),
        "DESCRIBE should return triples for the container"
    );
    // Verify the DESCRIBE includes type, status, and name triples
    let predicates: Vec<&str> = result.bindings.iter()
        .filter_map(|b| b.get("predicate").and_then(|v| v.get("value")).and_then(|v| v.as_str()))
        .collect();
    assert!(
        predicates.contains(&RDF_TYPE),
        "DESCRIBE should include rdf:type"
    );
    assert!(
        predicates.contains(&format!("{PICLOUD_NS}status").as_str()),
        "DESCRIBE should include picloud:status"
    );
}
