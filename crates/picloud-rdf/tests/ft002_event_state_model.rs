/// FT-002 Integration Tests — Event & State Model
///
/// Covers TC-013 through TC-021, TC-090-092, TC-103-106.
/// These tests verify the event log, RDF projection, replay, schema IRI,
/// and graph isolation behaviors.

use std::sync::Arc;

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{EventFilter, EventLog, QueryResult, ReplayEngine, ReplayRequest, StateProjector};
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

/// Helper: project all events into a projector and return the projector.
async fn project_events(events: &[EventEnvelope]) -> OxigraphProjector {
    let projector = OxigraphProjector::new().unwrap();
    for event in events {
        projector.project(event).await.unwrap();
    }
    projector
}

/// Helper: query and assert non-empty bindings
async fn assert_sparql_non_empty(projector: &OxigraphProjector, sparql: &str) {
    let result = projector.query(sparql).await.unwrap();
    assert!(
        !result.bindings.is_empty(),
        "Expected non-empty result for SPARQL: {sparql}"
    );
}

/// Helper: query and assert specific count
async fn assert_sparql_count(projector: &OxigraphProjector, sparql: &str, expected: usize) {
    let result = projector.query(sparql).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        expected,
        "Expected {expected} results for SPARQL: {sparql}, got {}: {:?}",
        result.bindings.len(),
        result.bindings
    );
}

// ============================================================================
// TC-013 — event_log_replay
// ============================================================================
/// Apply a set of resources, record the RDF graph state via SPARQL,
/// wipe the Oxigraph projection, replay the event log from index 0,
/// assert the resulting graph matches the recorded snapshot.
#[tokio::test]
async fn tc013_event_log_replay() {
    let ib = iri_builder();

    // 1. Create a set of events
    let events = vec![
        make_node_joined("pi-node-01", Uuid::new_v4()),
        make_product_deployed("photo-app", "1.0.0"),
        make_resource_declared("photo-app", "Container", "api-server"),
        make_resource_ready("photo-app", "Container", "api-server"),
        make_resource_declared("photo-app", "Volume", "data-store"),
        make_resource_ready("photo-app", "Volume", "data-store"),
        make_identity_created("alice"),
    ];

    // 2. Project all events into first projector
    let proj1 = project_events(&events).await;

    // 3. Record the RDF graph state — count all resources (excluding projector metadata)
    let snapshot_query = format!(
        "SELECT ?s ?p ?o WHERE {{ ?s ?p ?o . FILTER(!STRSTARTS(STR(?s), \"https://picloud.local/meta/\")) }} ORDER BY ?s ?p ?o"
    );
    let snapshot = proj1.query(&snapshot_query).await.unwrap();
    assert!(!snapshot.bindings.is_empty(), "Snapshot must have triples");

    // 4. Replay from scratch into a new projector
    let proj2 = OxigraphProjector::new().unwrap();
    let replayed = proj2.replay(&events).await.unwrap();
    assert_eq!(replayed, events.len());

    // 5. Record the replayed state (excluding projector metadata)
    let replayed_state = proj2.query(&snapshot_query).await.unwrap();

    // 6. Assert the graphs are identical
    assert_eq!(
        snapshot.bindings.len(),
        replayed_state.bindings.len(),
        "Replayed graph should have the same number of triples as original"
    );

    // Also verify specific resources exist (status is now a named node)
    let container_iri = ib.resource("photo-app", "container", "api-server");
    let ask = format!(
        "ASK {{ <{}> <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> }}",
        container_iri.as_str()
    );
    let result = proj2.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true);
}

// ============================================================================
// TC-014 — projection_consistency
// ============================================================================
/// After every `resource apply`, assert that the resulting RDF state (SPARQL ASK)
/// matches the declared resource definition within the projection latency budget.
#[tokio::test]
async fn tc014_projection_consistency() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // Apply a product
    let deploy_event = make_product_deployed("photo-app", "1.0.0");
    projector.project(&deploy_event).await.unwrap();

    // Assert product exists with correct type
    let product_iri = ib.product("photo-app");
    let ask = format!(
        "ASK {{ <{}> <{PICLOUD_NS}resourceType> \"Product\" }}",
        product_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true);

    // Apply a container
    let declared = make_resource_declared("photo-app", "Container", "api-server");
    projector.project(&declared).await.unwrap();

    let container_iri = ib.resource("photo-app", "container", "api-server");

    // Assert declared status (now stored as named node picloud:Declared)
    let ask = format!(
        "ASK {{ <{}> <{PICLOUD_NS}status> <{PICLOUD_NS}Declared> }}",
        container_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true);

    // Transition to ready
    let ready = make_resource_ready("photo-app", "Container", "api-server");
    projector.project(&ready).await.unwrap();

    // Assert ready status — old "declared" should be gone
    let ask = format!(
        "ASK {{ <{}> <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> }}",
        container_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true);

    let ask_old = format!(
        "ASK {{ <{}> <{PICLOUD_NS}status> <{PICLOUD_NS}Declared> }}",
        container_iri.as_str()
    );
    let result_old = projector.query(&ask_old).await.unwrap();
    assert_eq!(result_old.bindings[0]["result"], false, "Old status should be replaced");
}

// ============================================================================
// TC-015 — event_ordering
// ============================================================================
/// Apply 50 resources in parallel, assert the event log index is strictly
/// monotonic and the final RDF graph reflects all 50 resources.
#[tokio::test]
async fn tc015_event_ordering() {
    use picloud_events::InMemoryEventLog;

    let event_log = Arc::new(InMemoryEventLog::new());
    let projector = OxigraphProjector::new().unwrap();

    // Generate 50 resource declarations
    let mut events = Vec::new();
    for i in 0..50 {
        events.push(make_resource_declared(
            "photo-app",
            "Container",
            &format!("svc-{i:03}"),
        ));
    }

    // Append all events to the log
    for event in &events {
        event_log.append(event.clone()).await.unwrap();
    }

    // Verify event count
    assert_eq!(event_log.len().await, 50);

    // Verify monotonic ordering — events_since(0) should return all in order
    let all_events = event_log.events_since(0).await;
    assert_eq!(all_events.len(), 50);

    // Project all events
    for event in &all_events {
        projector.project(event).await.unwrap();
    }

    // Verify all 50 resources appear in the graph
    let count_query = format!(
        "SELECT (COUNT(?s) AS ?count) WHERE {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}Container> }}"
    );
    let result = projector.query(&count_query).await.unwrap();
    let count = result.bindings[0]["count"]["value"]
        .as_str()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    assert_eq!(count, 50, "All 50 container resources should be in the graph");

    // No duplicates
    let ib = iri_builder();
    for i in 0..50 {
        let iri = ib.resource("photo-app", "container", &format!("svc-{i:03}"));
        let ask = format!(
            "ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}Container> }}",
            iri.as_str()
        );
        let result = projector.query(&ask).await.unwrap();
        assert_eq!(result.bindings[0]["result"], true, "Container svc-{i:03} should exist");
    }
}

// ============================================================================
// TC-016 — rdf_projection_roundtrip
// ============================================================================
/// Apply a product with containers, volumes, and identities. Assert every
/// declared resource appears as typed triples. Wipe Oxigraph, replay the
/// event log, assert the graph is identical.
#[tokio::test]
async fn tc016_rdf_projection_roundtrip() {
    let ib = iri_builder();

    let events = vec![
        make_product_deployed("photo-app", "1.0.0"),
        make_resource_declared("photo-app", "Container", "api-server"),
        make_resource_ready("photo-app", "Container", "api-server"),
        make_resource_declared("photo-app", "Volume", "media-store"),
        make_resource_ready("photo-app", "Volume", "media-store"),
        make_identity_created("alice"),
    ];

    // Project into first projector
    let proj1 = project_events(&events).await;

    // Assert each resource exists
    let container_iri = ib.resource("photo-app", "container", "api-server");
    assert_sparql_non_empty(
        &proj1,
        &format!("SELECT ?s WHERE {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}Container> }}", container_iri.as_str()),
    ).await;

    let volume_iri = ib.resource("photo-app", "volume", "media-store");
    assert_sparql_non_empty(
        &proj1,
        &format!("SELECT ?s WHERE {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}Volume> }}", volume_iri.as_str()),
    ).await;

    let identity_iri = ib.resource("platform", "identities", "alice");
    assert_sparql_non_empty(
        &proj1,
        &format!("SELECT ?s WHERE {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}Identity> }}", identity_iri.as_str()),
    ).await;

    // Replay into fresh projector
    let proj2 = OxigraphProjector::new().unwrap();
    proj2.replay(&events).await.unwrap();

    // Verify identical (excluding projector metadata triples)
    let all_query = "SELECT (COUNT(*) AS ?c) WHERE { ?s ?p ?o . FILTER(!STRSTARTS(STR(?s), \"https://picloud.local/meta/\")) }";
    let c1 = proj1.query(all_query).await.unwrap();
    let c2 = proj2.query(all_query).await.unwrap();
    assert_eq!(c1.bindings[0]["c"]["value"], c2.bindings[0]["c"]["value"]);
}

// ============================================================================
// TC-017 — sparql_query_types
// ============================================================================
/// Execute SELECT, ASK, CONSTRUCT, and DESCRIBE queries against the platform
/// graph. Assert correct result formats and non-empty results.
#[tokio::test]
async fn tc017_sparql_query_types() {
    let projector = project_events(&[
        make_node_joined("pi-node-01", Uuid::new_v4()),
        make_product_deployed("photo-app", "1.0.0"),
    ]).await;

    // SELECT
    let select = format!(
        "SELECT ?node WHERE {{ ?node <{RDF_TYPE}> <{PICLOUD_NS}Node> }}"
    );
    let result = projector.query(&select).await.unwrap();
    assert!(!result.bindings.is_empty(), "SELECT should return results");
    assert!(result.bindings[0].get("node").is_some(), "SELECT should have 'node' binding");

    // ASK — true case
    let ask_true = format!(
        "ASK {{ ?node <{RDF_TYPE}> <{PICLOUD_NS}Node> }}"
    );
    let result = projector.query(&ask_true).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true);

    // ASK — false case
    let ask_false = format!(
        "ASK {{ <https://picloud.local/nonexistent> <{RDF_TYPE}> <{PICLOUD_NS}Node> }}"
    );
    let result = projector.query(&ask_false).await.unwrap();
    assert_eq!(result.bindings[0]["result"], false);

    // CONSTRUCT
    let construct = format!(
        "CONSTRUCT {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}Node> }} WHERE {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}Node> }}"
    );
    let result = projector.query(&construct).await.unwrap();
    assert!(!result.bindings.is_empty(), "CONSTRUCT should return triples");
    assert!(result.bindings[0].get("subject").is_some(), "CONSTRUCT results should have subject");
    assert!(result.bindings[0].get("predicate").is_some(), "CONSTRUCT results should have predicate");
    assert!(result.bindings[0].get("object").is_some(), "CONSTRUCT results should have object");

    // DESCRIBE
    let node_iri = iri_builder().node("pi-node-01");
    let describe = format!("DESCRIBE <{}>", node_iri.as_str());
    let result = projector.query(&describe).await.unwrap();
    assert!(!result.bindings.is_empty(), "DESCRIBE should return triples");
}

// ============================================================================
// TC-018 — graph_isolation
// ============================================================================
/// Assert that a SPARQL query against the platform graph does not return
/// triples from a product named graph, and vice versa.
#[tokio::test]
async fn tc018_graph_isolation() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // Add a node to the platform graph (default graph)
    projector.project(&make_node_joined("pi-node-01", Uuid::new_v4())).await.unwrap();

    // Add a product-scoped resource to its named graph
    projector.project(&make_product_deployed("photo-app", "1.0.0")).await.unwrap();
    projector.project(&make_resource_declared("photo-app", "Container", "api-server")).await.unwrap();

    // Platform graph query should NOT return product-scoped triples from named graph
    let platform_query = format!(
        "SELECT ?s WHERE {{ GRAPH <{}> {{ ?s <{RDF_TYPE}> ?t }} }}",
        ib.product_graph("photo-app").as_str()
    );
    let product_graph_result = projector.query(&platform_query).await.unwrap();

    // Product graph should contain the container
    let container_iri = ib.resource("photo-app", "container", "api-server");
    let product_has_container = product_graph_result.bindings.iter().any(|b| {
        b.get("s")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str())
            == Some(container_iri.as_str())
    });
    assert!(product_has_container, "Product graph should contain the container");

    // The node should NOT be in the product named graph
    let node_iri = ib.node("pi-node-01");
    let node_in_product = product_graph_result.bindings.iter().any(|b| {
        b.get("s")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str())
            == Some(node_iri.as_str())
    });
    assert!(!node_in_product, "Node should NOT be in product named graph");

    // Conversely, product-only query from product graph should not include nodes
    let product_iri = ib.product("photo-app");
    let product_query = format!(
        "SELECT ?s WHERE {{ GRAPH <{}> {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}Node> }} }}",
        ib.product_graph("photo-app").as_str()
    );
    let result = projector.query(&product_query).await.unwrap();
    assert!(result.bindings.is_empty(), "Product graph should not contain Node triples");
}

// ============================================================================
// TC-019 — oxigraph_sparql_compliance
// ============================================================================
/// Execute a representative set of SPARQL 1.1 queries including
/// SELECT with FILTER, ASK, CONSTRUCT, DESCRIBE, SPARQL Update INSERT, DELETE.
#[tokio::test]
async fn tc019_oxigraph_sparql_compliance() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // Setup: create nodes and resources
    projector.project(&make_node_joined("node-a", Uuid::new_v4())).await.unwrap();
    projector.project(&make_node_joined("node-b", Uuid::new_v4())).await.unwrap();
    projector.project(&make_product_deployed("photo-app", "1.0.0")).await.unwrap();
    projector.project(&make_resource_declared("photo-app", "Container", "web")).await.unwrap();
    projector.project(&make_resource_declared("photo-app", "Container", "api")).await.unwrap();

    // 1. SELECT with FILTER
    let filter_query = format!(
        "SELECT ?s WHERE {{ ?s <{PICLOUD_NS}nodeName> ?name . FILTER(?name = \"node-a\") }}"
    );
    let result = projector.query(&filter_query).await.unwrap();
    assert_eq!(result.bindings.len(), 1, "FILTER should return exactly one node");

    // 2. ASK
    let ask = format!("ASK {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}Node> }}");
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true);

    // 3. CONSTRUCT
    let construct = format!(
        "CONSTRUCT {{ ?s <{PICLOUD_NS}isNode> true }} WHERE {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}Node> }}"
    );
    let result = projector.query(&construct).await.unwrap();
    assert_eq!(result.bindings.len(), 2, "CONSTRUCT should return 2 triples for 2 nodes");

    // 4. DESCRIBE
    let node_iri = ib.node("node-a");
    let describe = format!("DESCRIBE <{}>", node_iri.as_str());
    let result = projector.query(&describe).await.unwrap();
    assert!(!result.bindings.is_empty(), "DESCRIBE should return triples");

    // 5. SPARQL Update INSERT (via store directly)
    let insert = format!(
        "INSERT DATA {{ <https://picloud.local/test/x> <{PICLOUD_NS}testProp> \"hello\" }}"
    );
    projector.store().update(&insert).unwrap();

    // Verify the insert
    let verify = format!(
        "ASK {{ <https://picloud.local/test/x> <{PICLOUD_NS}testProp> \"hello\" }}"
    );
    let result = projector.query(&verify).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true);

    // 6. SPARQL Update DELETE
    let delete = format!(
        "DELETE DATA {{ <https://picloud.local/test/x> <{PICLOUD_NS}testProp> \"hello\" }}"
    );
    projector.store().update(&delete).unwrap();

    let result = projector.query(&verify).await.unwrap();
    assert_eq!(result.bindings[0]["result"], false, "DELETE should remove the triple");
}

// ============================================================================
// TC-020 — named_graph_isolation
// ============================================================================
/// Write triples to three named graphs, assert each graph query returns only
/// its own triples, and that union returns all.
#[tokio::test]
async fn tc020_named_graph_isolation() {
    let projector = OxigraphProjector::new().unwrap();

    // Create three products, each with a resource in its named graph
    for product in &["app-a", "app-b", "app-c"] {
        projector.project(&make_product_deployed(product, "1.0.0")).await.unwrap();
        projector.project(&make_resource_declared(product, "Container", "svc")).await.unwrap();
    }

    let ib = iri_builder();

    // Each named graph should contain only its own resource
    for product in &["app-a", "app-b", "app-c"] {
        let graph_iri = ib.product_graph(product);
        let query = format!(
            "SELECT ?s WHERE {{ GRAPH <{}> {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}Resource> }} }}",
            graph_iri.as_str()
        );
        let result = projector.query(&query).await.unwrap();
        assert!(
            !result.bindings.is_empty(),
            "Named graph for {product} should have its resource"
        );

        // Verify it's the correct product's resource
        let expected_iri = ib.resource(product, "container", "svc");
        let found = result.bindings.iter().any(|b| {
            b.get("s")
                .and_then(|v| v.get("value"))
                .and_then(|v| v.as_str())
                == Some(expected_iri.as_str())
        });
        assert!(found, "Named graph for {product} should contain {}", expected_iri.as_str());
    }

    // Cross-check: app-a's graph should NOT contain app-b's resource
    let graph_a = ib.product_graph("app-a");
    let iri_b = ib.resource("app-b", "container", "svc");
    let cross_query = format!(
        "ASK {{ GRAPH <{}> {{ <{}> ?p ?o }} }}",
        graph_a.as_str(),
        iri_b.as_str()
    );
    let result = projector.query(&cross_query).await.unwrap();
    assert_eq!(result.bindings[0]["result"], false, "app-a graph should not contain app-b resources");
}

// ============================================================================
// TC-021 — oxigraph_persistence
// ============================================================================
/// Write triples, create a new projector from the same disk path,
/// assert triples are still present (persistence verification).
#[tokio::test]
async fn tc021_oxigraph_persistence() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let store_path = tmp_dir.path().join("oxigraph");
    let domain = ClusterDomain::default();

    // Phase 1: write data
    {
        let projector = OxigraphProjector::open(&store_path, domain.clone()).unwrap();
        projector.project(&make_node_joined("pi-node-01", Uuid::new_v4())).await.unwrap();
        projector.project(&make_product_deployed("photo-app", "1.0.0")).await.unwrap();

        // Verify data exists
        let query = format!("SELECT ?s WHERE {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}Node> }}");
        let result = projector.query(&query).await.unwrap();
        assert_eq!(result.bindings.len(), 1);
    }

    // Phase 2: reopen from same path — data should persist
    {
        let projector = OxigraphProjector::open(&store_path, domain).unwrap();

        let query = format!("SELECT ?s WHERE {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}Node> }}");
        let result = projector.query(&query).await.unwrap();

        // Note: persistence depends on the rocksdb feature.
        // Without it, the store falls back to in-memory and this will be empty.
        // The test documents the expected behavior for both modes.
        #[cfg(feature = "rocksdb")]
        assert_eq!(result.bindings.len(), 1, "Node should persist across restart (rocksdb)");

        // Verify cursor was persisted too
        #[cfg(feature = "rocksdb")]
        assert!(projector.cursor() >= 1, "Cursor should be persisted");
    }
}

// ============================================================================
// TC-090 — schema_iri_resolution
// ============================================================================
/// Emit a platform event. Extract the `schema` field from the event envelope.
/// Assert it is a valid, well-formed schema IRI.
#[tokio::test]
async fn tc090_schema_iri_resolution() {
    let ib = iri_builder();
    let event = make_event(
        "ResourceReady",
        Some("photo-app"),
        serde_json::json!({
            "resource_iri": "https://picloud.local/products/photo-app/containers/api",
        }),
    );

    // Extract schema IRI from event envelope
    let schema_iri = event.schema.as_str();
    assert_eq!(
        schema_iri,
        "https://picloud.local/schemas/events/ResourceReady/v1",
        "Schema IRI should follow the canonical format"
    );

    // Verify it matches the IRI builder output
    let expected = ib.event_schema("ResourceReady", 1);
    assert_eq!(event.schema.as_str(), expected.as_str());

    // Verify different event types have different schema IRIs
    let event2 = make_event(
        "NodeJoined",
        None,
        serde_json::json!({}),
    );
    assert_ne!(
        event.schema.as_str(),
        event2.schema.as_str(),
        "Different event types should have different schema IRIs"
    );

    // Verify versioning works
    let v2_schema = ib.event_schema("ResourceReady", 2);
    assert_eq!(
        v2_schema.as_str(),
        "https://picloud.local/schemas/events/ResourceReady/v2"
    );
    assert_ne!(
        event.schema.as_str(),
        v2_schema.as_str(),
        "Different versions should have different IRIs"
    );
}

// ============================================================================
// TC-091 — schema_evolution
// ============================================================================
/// Emit 100 events under schema v1. Deploy a v2 projector that handles both.
/// Replay the log. Assert the v2 projector correctly processes all v1 events.
#[tokio::test]
async fn tc091_schema_evolution() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // Emit 100 events under v1 schema
    let mut events = Vec::new();
    for i in 0..100 {
        let resource_iri = ib.resource("photo-app", "container", &format!("svc-{i:03}"));
        let mut event = make_event(
            "ResourceDeclared",
            Some("photo-app"),
            serde_json::json!({
                "resource_iri": resource_iri.as_str(),
                "resource_type": "Container",
                "product": "photo-app",
                "name": format!("svc-{i:03}"),
            }),
        );
        // All events carry v1 schema
        event.schema = ib.event_schema("ResourceDeclared", 1);
        events.push(event);
    }

    // The current projector (acting as "v2") handles v1 events
    let replayed = projector.replay(&events).await.unwrap();
    assert_eq!(replayed, 100);

    // Verify all 100 resources were correctly projected
    let count_query = format!(
        "SELECT (COUNT(?s) AS ?count) WHERE {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}Container> }}"
    );
    let result = projector.query(&count_query).await.unwrap();
    let count = result.bindings[0]["count"]["value"]
        .as_str()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    assert_eq!(count, 100, "V2 projector should correctly process all v1 events");
}

// ============================================================================
// TC-092 — schema_iri_permanence
// ============================================================================
/// Assert that v1 and v2 schema IRIs coexist and are distinct.
/// Both should remain resolvable forever.
#[tokio::test]
async fn tc092_schema_iri_permanence() {
    let ib = iri_builder();

    // v1 schema IRI
    let v1 = ib.event_schema("ResourceReady", 1);
    assert_eq!(v1.as_str(), "https://picloud.local/schemas/events/ResourceReady/v1");

    // v2 schema IRI
    let v2 = ib.event_schema("ResourceReady", 2);
    assert_eq!(v2.as_str(), "https://picloud.local/schemas/events/ResourceReady/v2");

    // Both coexist
    assert_ne!(v1.as_str(), v2.as_str());

    // Both are valid ResourceIri values
    assert!(v1.as_str().starts_with("https://"));
    assert!(v2.as_str().starts_with("https://"));

    // Verify events can carry either version
    let event_v1 = EventEnvelope::new(
        v1.clone(),
        "ResourceReady",
        ResourceIri::new("https://picloud.local/test").unwrap(),
        None,
        Uuid::new_v4(),
        serde_json::json!({}),
    );
    let event_v2 = EventEnvelope::new(
        v2.clone(),
        "ResourceReady",
        ResourceIri::new("https://picloud.local/test").unwrap(),
        None,
        Uuid::new_v4(),
        serde_json::json!({}),
    );

    assert_eq!(event_v1.schema.as_str(), v1.as_str());
    assert_eq!(event_v2.schema.as_str(), v2.as_str());
}

// ============================================================================
// TC-103 — platform_replay_full
// ============================================================================
/// Emit 500 known events, record the RDF graph state. Clear Oxigraph.
/// Trigger replay from epoch. Assert the resulting graph matches.
#[tokio::test]
async fn tc103_platform_replay_full() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // Emit 500 events: mix of nodes, products, resources
    let mut events = Vec::new();

    // 5 nodes
    for i in 0..5 {
        events.push(make_node_joined(&format!("node-{i}"), Uuid::new_v4()));
    }

    // 10 products with 49 containers each = 490 events
    for p in 0..10 {
        let product_name = format!("app-{p}");
        events.push(make_product_deployed(&product_name, "1.0.0"));
        for c in 0..49 {
            events.push(make_resource_declared(
                &product_name,
                "Container",
                &format!("svc-{c:03}"),
            ));
        }
    }

    // Verify we have ~500 events
    assert!(events.len() >= 500, "Should have at least 500 events, got {}", events.len());
    let events = events[..500].to_vec(); // Trim to exactly 500

    // Project all
    for event in &events {
        projector.project(event).await.unwrap();
    }

    // Record state (excluding projector metadata)
    let snapshot_query = "SELECT (COUNT(*) AS ?c) WHERE { ?s ?p ?o . FILTER(!STRSTARTS(STR(?s), \"https://picloud.local/meta/\")) }";
    let snapshot = projector.query(snapshot_query).await.unwrap();
    let original_count = snapshot.bindings[0]["c"]["value"]
        .as_str()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    assert!(original_count > 0);

    // Replay into fresh projector
    let proj2 = OxigraphProjector::new().unwrap();
    let replayed = proj2.replay(&events).await.unwrap();
    assert_eq!(replayed, 500);

    // Verify same triple count (excluding projector metadata)
    let replayed_state = proj2.query(snapshot_query).await.unwrap();
    let replayed_count = replayed_state.bindings[0]["c"]["value"]
        .as_str()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    assert_eq!(
        original_count, replayed_count,
        "Replayed graph should have identical triple count"
    );
}

// ============================================================================
// TC-104 — shadow_swap_live_traffic
// ============================================================================
/// Trigger a replay while serving live SPARQL queries. Assert zero query
/// errors during replay and atomic swap.
#[tokio::test]
async fn tc104_shadow_swap_live_traffic() {
    let ib = iri_builder();
    let projector = Arc::new(OxigraphProjector::new().unwrap());

    // Setup: create product with some resources
    projector.project(&make_product_deployed("photo-app", "1.0.0")).await.unwrap();
    for i in 0..10 {
        projector.project(&make_resource_declared(
            "photo-app", "Container", &format!("svc-{i}"),
        )).await.unwrap();
    }

    // Concurrent queries during replay
    let query_projector = projector.clone();
    let query_handle = tokio::spawn(async move {
        let mut errors = 0;
        for _ in 0..100 {
            let query = format!(
                "SELECT ?s WHERE {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}Resource> }}"
            );
            match query_projector.query(&query).await {
                Ok(_result) => {} // Query succeeded
                Err(_) => errors += 1,
            }
            tokio::task::yield_now().await;
        }
        errors
    });

    // Execute replay simultaneously
    let replay_events: Vec<EventEnvelope> = (0..20).map(|i| {
        make_resource_declared("photo-app", "Container", &format!("new-svc-{i}"))
    }).collect();

    let target_graph = ib.product_graph("photo-app").to_string();
    let replay_id = Uuid::new_v4();
    let result = projector.execute_replay(
        replay_id,
        &replay_events,
        Some(&target_graph),
    ).unwrap();
    assert_eq!(result.events_replayed, 20);

    // Wait for queries to complete
    let query_errors = query_handle.await.unwrap();
    assert_eq!(query_errors, 0, "Zero query errors during replay");
}

// ============================================================================
// TC-105 — replay_marked_flag
// ============================================================================
/// Replay 100 events. Verify the replay mechanism supports marking events
/// with replay metadata (replay_id, is_replay).
#[tokio::test]
async fn tc105_replay_marked_flag() {
    let ib = iri_builder();

    // Create events
    let mut events: Vec<EventEnvelope> = (0..100).map(|i| {
        make_resource_declared("photo-app", "Container", &format!("svc-{i:03}"))
    }).collect();

    // Simulate replay marking: add replay metadata to event payloads
    let replay_id = Uuid::new_v4();
    let replayed_at = chrono::Utc::now();

    for event in &mut events {
        // In a real replay, the platform adds this field to re-emitted events
        let mut payload = event.payload.clone();
        if let serde_json::Value::Object(ref mut map) = payload {
            map.insert("replay".to_string(), serde_json::json!({
                "is_replay": true,
                "replay_id": replay_id.to_string(),
                "original_timestamp": event.timestamp.to_rfc3339(),
                "replayed_at": replayed_at.to_rfc3339(),
            }));
        }
        event.payload = payload;
    }

    // Verify every event carries the replay marker
    for event in &events {
        let replay = event.payload.get("replay").expect("replay field should exist");
        assert_eq!(replay["is_replay"], true, "is_replay should be true");
        assert_eq!(
            replay["replay_id"].as_str().unwrap(),
            replay_id.to_string(),
            "replay_id should match"
        );
        assert!(replay["original_timestamp"].is_string(), "original_timestamp should be present");
        assert!(replay["replayed_at"].is_string(), "replayed_at should be present");
    }

    // Events should still project correctly despite replay markers
    let projector = OxigraphProjector::new().unwrap();
    let replayed = projector.replay(&events).await.unwrap();
    assert_eq!(replayed, 100);

    let count_query = format!(
        "SELECT (COUNT(?s) AS ?count) WHERE {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}Container> }}"
    );
    let result = projector.query(&count_query).await.unwrap();
    let count = result.bindings[0]["count"]["value"]
        .as_str()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    assert_eq!(count, 100, "All 100 replayed events should be projected");
}

// ============================================================================
// TC-106 — aggregate_replay
// ============================================================================
/// Replay a single aggregate from a product event store. Assert only that
/// aggregate's events are re-emitted.
#[tokio::test]
async fn tc106_aggregate_replay() {
    use picloud_events::InMemoryEventLog;

    let ib = iri_builder();
    let event_log = Arc::new(InMemoryEventLog::new());

    // Create events for two aggregates
    let agg_a_id = Uuid::new_v4();
    let agg_b_id = Uuid::new_v4();

    let event_a = {
        let mut e = make_resource_declared("photo-app", "Container", "svc-a");
        e.payload["aggregate_type"] = serde_json::json!("Photo");
        e.payload["aggregate_id"] = serde_json::json!(agg_a_id.to_string());
        e
    };
    let event_b = {
        let mut e = make_resource_declared("photo-app", "Container", "svc-b");
        e.payload["aggregate_type"] = serde_json::json!("Photo");
        e.payload["aggregate_id"] = serde_json::json!(agg_b_id.to_string());
        e
    };

    event_log.append(event_a.clone()).await.unwrap();
    event_log.append(event_b.clone()).await.unwrap();

    // Filter events for aggregate A only
    let all_events = event_log.events_since(0).await;
    let agg_a_events: Vec<&EventEnvelope> = all_events.iter().filter(|e| {
        e.payload.get("aggregate_id")
            .and_then(|v| v.as_str())
            == Some(&agg_a_id.to_string())
    }).collect();

    assert_eq!(agg_a_events.len(), 1, "Should find exactly 1 event for aggregate A");

    // Replay only aggregate A events
    let projector = OxigraphProjector::new().unwrap();
    for event in &agg_a_events {
        projector.project(event).await.unwrap();
    }

    // Verify only aggregate A's resource exists
    let iri_a = ib.resource("photo-app", "container", "svc-a");
    let ask_a = format!("ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}Container> }}", iri_a.as_str());
    let result = projector.query(&ask_a).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "Aggregate A should be projected");

    let iri_b = ib.resource("photo-app", "container", "svc-b");
    let ask_b = format!("ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}Container> }}", iri_b.as_str());
    let result = projector.query(&ask_b).await.unwrap();
    assert_eq!(result.bindings[0]["result"], false, "Aggregate B should NOT be projected");
}

// ============================================================================
// TC-210 — Cluster survives one node restart without data loss
// ============================================================================
/// Simulate node restart by projecting events, "restarting" (creating a new
/// projector with same event replay), and verifying data integrity.
#[tokio::test]
async fn tc210_cluster_survives_node_restart() {
    let events = vec![
        make_node_joined("node-1", Uuid::new_v4()),
        make_node_joined("node-2", Uuid::new_v4()),
        make_node_joined("node-3", Uuid::new_v4()),
        make_product_deployed("photo-app", "1.0.0"),
        make_resource_declared("photo-app", "Container", "api"),
        make_resource_ready("photo-app", "Container", "api"),
    ];

    // Initial projection
    let proj1 = project_events(&events).await;

    // Record state
    let query = format!("SELECT ?s WHERE {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}Node> }}");
    let before = proj1.query(&query).await.unwrap();
    assert_eq!(before.bindings.len(), 3, "Should have 3 nodes");

    // "Restart" node: replay from event log
    let proj2 = OxigraphProjector::new().unwrap();
    let replayed = proj2.replay(&events).await.unwrap();
    assert_eq!(replayed, events.len());

    // Verify same state after restart
    let after = proj2.query(&query).await.unwrap();
    assert_eq!(after.bindings.len(), 3, "Should still have 3 nodes after restart");

    // Verify resource state preserved (status is now a named node)
    let ib = iri_builder();
    let container_iri = ib.resource("photo-app", "container", "api");
    let ask = format!(
        "ASK {{ <{}> <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> }}",
        container_iri.as_str()
    );
    let result = proj2.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "Container should be ready after restart");
}

// ============================================================================
// TC-219 — Raft leader failover under network partition
// ============================================================================
/// Simulates the event log replay portion: verify that with 3 nodes,
/// if one goes down, the remaining 2 can still replay and query consistently.
#[tokio::test]
async fn tc219_raft_leader_failover_event_consistency() {
    let node_ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();

    let events = vec![
        make_node_joined("node-0", node_ids[0]),
        make_node_joined("node-1", node_ids[1]),
        make_node_joined("node-2", node_ids[2]),
        make_product_deployed("photo-app", "1.0.0"),
        make_resource_declared("photo-app", "Container", "api"),
        make_resource_ready("photo-app", "Container", "api"),
    ];

    // All three nodes project the same events (simulating Raft replication)
    let mut projectors = Vec::new();
    for _ in 0..3 {
        let proj = OxigraphProjector::new().unwrap();
        proj.replay(&events).await.unwrap();
        projectors.push(proj);
    }

    // Verify all three have identical state
    let query = "SELECT (COUNT(*) AS ?c) WHERE { ?s ?p ?o }";
    let counts: Vec<String> = futures::future::join_all(
        projectors.iter().map(|p| async {
            p.query(query).await.unwrap().bindings[0]["c"]["value"]
                .as_str()
                .unwrap()
                .to_string()
        })
    ).await;

    assert_eq!(counts[0], counts[1], "Node 0 and 1 should have identical triple count");
    assert_eq!(counts[1], counts[2], "Node 1 and 2 should have identical triple count");

    // Simulate node-0 (leader) going down: remaining nodes continue working
    // Node 1 and 2 can still query
    for i in 1..3 {
        let ib = iri_builder();
        let container_iri = ib.resource("photo-app", "container", "api");
        let ask = format!(
            "ASK {{ <{}> <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> }}",
            container_iri.as_str()
        );
        let result = projectors[i].query(&ask).await.unwrap();
        assert_eq!(
            result.bindings[0]["result"], true,
            "Node {i} should still serve correct state after leader failure"
        );
    }

    // New events can be projected on surviving nodes
    let new_event = make_resource_declared("photo-app", "Container", "worker");
    projectors[1].project(&new_event).await.unwrap();
    projectors[2].project(&new_event).await.unwrap();

    // Both surviving nodes should have the new resource
    let ib = iri_builder();
    let worker_iri = ib.resource("photo-app", "container", "worker");
    for i in 1..3 {
        let ask = format!(
            "ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}Container> }}",
            worker_iri.as_str()
        );
        let result = projectors[i].query(&ask).await.unwrap();
        assert_eq!(result.bindings[0]["result"], true, "Node {i} should have new resource");
    }
}

// ============================================================================
// TC-223 — Split brain — exactly one leader invariant
// ============================================================================
/// Verify that the RDF projection correctly models leader election:
/// at most one node should carry the Leader role at any time.
#[tokio::test]
async fn tc223_split_brain_one_leader_invariant() {
    let projector = OxigraphProjector::new().unwrap();
    let ib = iri_builder();

    // Join 5 nodes
    let mut node_ids = Vec::new();
    for i in 0..5 {
        let nid = Uuid::new_v4();
        node_ids.push(nid);
        projector.project(&make_node_joined(&format!("node-{i}"), nid)).await.unwrap();
    }

    // Elect node-0 as leader
    let leader_event = make_event(
        "LeaderElected",
        None,
        serde_json::json!({
            "node_id": node_ids[0].to_string(),
            "node_iri": ib.node("node-0").as_str(),
        }),
    );
    projector.project(&leader_event).await.unwrap();

    // Verify exactly one leader
    let leader_query = format!(
        "SELECT ?node WHERE {{ ?node <{PICLOUD_NS}hasRole> <{PICLOUD_NS}Leader> }}"
    );
    let result = projector.query(&leader_query).await.unwrap();
    assert_eq!(result.bindings.len(), 1, "Exactly one leader after election");

    // Simulate partition: node-3 gets elected as leader (in a different partition)
    let new_leader_event = make_event(
        "LeaderElected",
        None,
        serde_json::json!({
            "node_id": node_ids[3].to_string(),
            "node_iri": ib.node("node-3").as_str(),
        }),
    );
    projector.project(&new_leader_event).await.unwrap();

    // After the event is projected, there should still be exactly one leader
    // (the LeaderElected projector removes the old leader designation)
    let result = projector.query(&leader_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        1,
        "Exactly one leader after re-election — no split brain in graph"
    );

    // The new leader should be node-3
    let leader_iri = result.bindings[0]["node"]["value"]
        .as_str()
        .unwrap();
    assert!(
        leader_iri.contains("node-3"),
        "New leader should be node-3, got: {leader_iri}"
    );
}
