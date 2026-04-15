/// FT-085 Integration Tests — Product discoverability — cluster SPARQL query
/// returns all Products, events, ontologies, capabilities, data products
///
/// Covers:
///   TC-284: Cluster SPARQL query returns all products, events, ontologies (scenario)
///   TC-341: Discoverability exit — SPARQL returns all products and ontologies (exit-criteria)
///
/// Verifies that a single cluster-wide SPARQL query can discover:
///   - All deployed products with their metadata (name, version, status)
///   - Ontology resources bound to products
///   - Event stores declared by products
///   - Product events projected into the RDF graph
///   - Capabilities declared across the cluster
///   - Data products declared across products and domains
///   - Cross-product discoverability without requiring product-scoped queries

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

fn make_ontology_declared(
    product: &str,
    name: &str,
    file_path: &str,
    format: &str,
    version: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let resource_iri = ib.resource(product, "ontology", name);
    let served_at = ib.product_ontology_versioned(product, version);
    make_event(
        "ResourceDeclared",
        Some(product),
        serde_json::json!({
            "resource_iri": resource_iri.as_str(),
            "resource_type": "Ontology",
            "product": product,
            "name": name,
            "file_path": file_path,
            "format": format,
            "served_at": served_at.as_str(),
            "version": version,
        }),
    )
}

fn make_event_store_declared(
    product: &str,
    store_name: &str,
    aggregates: serde_json::Value,
) -> EventEnvelope {
    let ib = iri_builder();
    let resource_iri = ib.resource(product, "event-store", store_name);
    make_event(
        "ResourceDeclared",
        Some(product),
        serde_json::json!({
            "resource_iri": resource_iri.as_str(),
            "resource_type": "EventStore",
            "product": product,
            "name": store_name,
            "aggregates": aggregates,
        }),
    )
}

fn make_resource_ready(product: &str, resource_type_slug: &str, name: &str) -> EventEnvelope {
    let ib = iri_builder();
    let resource_iri = ib.resource(product, resource_type_slug, name);
    make_event(
        "ResourceReady",
        Some(product),
        serde_json::json!({
            "resource_iri": resource_iri.as_str(),
        }),
    )
}

fn make_capability_declared(
    name: &str,
    version: &str,
    input_event: &str,
    output_event: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let capability_iri = ib.cluster_resource("capabilities", name);
    make_event(
        "CapabilityDeclared",
        None,
        serde_json::json!({
            "capability_iri": capability_iri.as_str(),
            "name": name,
            "version": version,
            "input_event": input_event,
            "output_event": output_event,
        }),
    )
}

fn make_capability_implementor_added(
    capability_name: &str,
    product_name: &str,
    version: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let capability_iri = ib.cluster_resource("capabilities", capability_name);
    let product_iri = ib.product(product_name);
    make_event(
        "CapabilityImplementorAdded",
        None,
        serde_json::json!({
            "capability_iri": capability_iri.as_str(),
            "capability_name": capability_name,
            "product_iri": product_iri.as_str(),
            "product_name": product_name,
            "version": version,
        }),
    )
}

fn dp_resource_iri(product: &str, dp_name: &str) -> String {
    let ib = iri_builder();
    let dp_graph_iri = ib.data_product_graph(product, dp_name);
    dp_graph_iri
        .as_str()
        .trim_end_matches("/graph")
        .to_string()
}

fn make_data_domain_declared(
    name: &str,
    steward: &str,
    sensitivity: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let domain_iri = ib.cluster_resource("data-domains", name);
    make_event(
        "DataDomainDeclared",
        None,
        serde_json::json!({
            "domain_iri": domain_iri.as_str(),
            "name": name,
            "steward": steward,
            "sensitivity": sensitivity,
        }),
    )
}

fn make_data_product_declared(
    product: &str,
    dp_name: &str,
    domain: &str,
    version: &str,
    max_age: Option<&str>,
) -> EventEnvelope {
    let dp_iri_str = dp_resource_iri(product, dp_name);
    let mut payload = serde_json::json!({
        "data_product_iri": dp_iri_str,
        "name": dp_name,
        "product": product,
        "domain": domain,
        "version": version,
    });
    if let Some(ma) = max_age {
        payload["max_age"] = serde_json::Value::String(ma.to_string());
    }
    make_event(
        "DataProductDeclared",
        Some(product),
        payload,
    )
}

fn make_product_event_appended(
    product: &str,
    store_name: &str,
    aggregate_type: &str,
    aggregate_id: &str,
    product_event_type: &str,
    data: serde_json::Value,
) -> EventEnvelope {
    let ib = iri_builder();
    let event_iri = ib.aggregate_stream(product, store_name, aggregate_type, aggregate_id);
    make_event(
        "ProductEventAppended",
        Some(product),
        serde_json::json!({
            "event_iri": format!("{}/{}", event_iri.as_str(), Uuid::new_v4()),
            "store_name": store_name,
            "aggregate_type": aggregate_type,
            "aggregate_id": aggregate_id,
            "product_event_type": product_event_type,
            "data": data,
        }),
    )
}

// ============================================================================
// TC-284 — Cluster SPARQL query returns all products, events, ontologies
// ============================================================================
/// Scenario test for FT-085:
///
/// Deploys a representative cluster state with multiple products, each owning
/// different resource types (ontologies, event stores, containers), plus
/// cluster-scoped capabilities, data domains, and data products. Then issues
/// cluster-wide (non-product-scoped) SPARQL queries and verifies that ALL
/// resource types are discoverable from a single query endpoint.
///
/// Steps:
///   1. Deploy three products (photo-app, chat-app, analytics-engine)
///   2. Declare ontologies for two products
///   3. Declare event stores for two products and append product events
///   4. Declare capabilities and link implementors
///   5. Declare data domains and data products
///   6. Run cluster-wide SPARQL queries to verify:
///      a. All 3 products are discoverable
///      b. All ontologies are discoverable
///      c. All event stores are discoverable
///      d. Product events are discoverable
///      e. All capabilities are discoverable
///      f. All data products are discoverable
///      g. A single union query discovers ALL resource types at once
#[tokio::test]
async fn tc284_cluster_sparql_query_returns_all_products_events_ontologies() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // =========================================================================
    // Step 1: Deploy three products
    // =========================================================================
    let products = [
        ("photo-app", "1.0.0"),
        ("chat-app", "2.0.0"),
        ("analytics-engine", "3.0.0"),
    ];
    for (name, version) in &products {
        projector
            .project(&make_product_deployed(name, version))
            .await
            .unwrap();
    }

    // =========================================================================
    // Step 2: Declare ontologies for photo-app and analytics-engine
    // =========================================================================
    let ontology_events = [
        make_ontology_declared(
            "photo-app",
            "photo-schema",
            "ontologies/photo-schema.ttl",
            "turtle",
            "1.0.0",
        ),
        make_ontology_declared(
            "analytics-engine",
            "metrics-schema",
            "ontologies/metrics-schema.ttl",
            "turtle",
            "3.0.0",
        ),
    ];
    for event in &ontology_events {
        projector.project(event).await.unwrap();
    }
    // Mark ontologies as ready
    projector
        .project(&make_resource_ready("photo-app", "ontology", "photo-schema"))
        .await
        .unwrap();
    projector
        .project(&make_resource_ready(
            "analytics-engine",
            "ontology",
            "metrics-schema",
        ))
        .await
        .unwrap();

    // =========================================================================
    // Step 3: Declare event stores and append product events
    // =========================================================================
    // photo-app event store with Photo aggregate
    projector
        .project(&make_event_store_declared(
            "photo-app",
            "photo-events",
            serde_json::json!([
                {"aggregate_type": "Photo", "schema_file": "schemas/photo.json"}
            ]),
        ))
        .await
        .unwrap();
    projector
        .project(&make_resource_ready(
            "photo-app",
            "event-store",
            "photo-events",
        ))
        .await
        .unwrap();

    // chat-app event store with Message aggregate
    projector
        .project(&make_event_store_declared(
            "chat-app",
            "chat-events",
            serde_json::json!([
                {"aggregate_type": "Message", "schema_file": "schemas/message.json"}
            ]),
        ))
        .await
        .unwrap();
    projector
        .project(&make_resource_ready(
            "chat-app",
            "event-store",
            "chat-events",
        ))
        .await
        .unwrap();

    // Append product events
    projector
        .project(&make_product_event_appended(
            "photo-app",
            "photo-events",
            "Photo",
            "photo-001",
            "PhotoUploaded",
            serde_json::json!({"filename": "sunset.jpg", "size_bytes": 4096}),
        ))
        .await
        .unwrap();
    projector
        .project(&make_product_event_appended(
            "chat-app",
            "chat-events",
            "Message",
            "msg-001",
            "MessageSent",
            serde_json::json!({"text": "Hello world", "channel": "general"}),
        ))
        .await
        .unwrap();

    // =========================================================================
    // Step 4: Declare capabilities and link implementors
    // =========================================================================
    projector
        .project(&make_capability_declared(
            "image-resize",
            "1.0.0",
            "PhotoUploaded",
            "PhotoResized",
        ))
        .await
        .unwrap();
    projector
        .project(&make_capability_declared(
            "sentiment-analysis",
            "2.0.0",
            "MessageSent",
            "SentimentScored",
        ))
        .await
        .unwrap();
    // Link photo-app as implementor of image-resize
    projector
        .project(&make_capability_implementor_added(
            "image-resize",
            "photo-app",
            "1.0.0",
        ))
        .await
        .unwrap();
    // Link analytics-engine as implementor of sentiment-analysis
    projector
        .project(&make_capability_implementor_added(
            "sentiment-analysis",
            "analytics-engine",
            "3.0.0",
        ))
        .await
        .unwrap();

    // =========================================================================
    // Step 5: Declare data domains and data products
    // =========================================================================
    projector
        .project(&make_data_domain_declared(
            "media",
            "https://picloud.local/identities/steward-media",
            "internal",
        ))
        .await
        .unwrap();
    projector
        .project(&make_data_domain_declared(
            "communications",
            "https://picloud.local/identities/steward-comms",
            "restricted",
        ))
        .await
        .unwrap();

    // photo-app data product in media domain
    projector
        .project(&make_data_product_declared(
            "photo-app",
            "photo-catalog",
            "media",
            "1.0.0",
            Some("PT1H"),
        ))
        .await
        .unwrap();
    // analytics-engine data product in communications domain
    projector
        .project(&make_data_product_declared(
            "analytics-engine",
            "chat-metrics",
            "communications",
            "1.0.0",
            Some("PT15M"),
        ))
        .await
        .unwrap();

    // =========================================================================
    // Step 6a: Verify ALL products are discoverable via cluster-wide SPARQL
    // =========================================================================
    let product_query = format!(
        "SELECT ?p ?name ?version WHERE {{ \
         ?p <{PICLOUD_NS}resourceType> \"Product\" ; \
            <{PICLOUD_NS}name> ?name ; \
            <{PICLOUD_NS}version> ?version \
         }} ORDER BY ?name"
    );
    let result = projector.query(&product_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        3,
        "Should discover all 3 products, got: {:?}",
        result.bindings
    );
    // Verify product names (sorted)
    assert_eq!(result.bindings[0]["name"]["value"], "analytics-engine");
    assert_eq!(result.bindings[1]["name"]["value"], "chat-app");
    assert_eq!(result.bindings[2]["name"]["value"], "photo-app");

    // =========================================================================
    // Step 6b: Verify ALL ontologies are discoverable
    // =========================================================================
    let ontology_query = format!(
        "SELECT ?o ?name ?format WHERE {{ \
         ?o <{RDF_TYPE}> <{PICLOUD_NS}Ontology> ; \
            <{PICLOUD_NS}filePath> ?fp . \
         OPTIONAL {{ ?o <{PICLOUD_NS}format> ?format }} \
         BIND(STR(?o) AS ?name) \
         }} ORDER BY ?name"
    );
    let result = projector.query(&ontology_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        2,
        "Should discover 2 ontologies, got: {:?}",
        result.bindings
    );

    // Verify individual ontology IRIs exist
    let photo_ontology_iri = ib.resource("photo-app", "ontology", "photo-schema");
    let ask_photo_ontology = format!(
        "ASK {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}Ontology> }}",
        iri = photo_ontology_iri.as_str()
    );
    let result = projector.query(&ask_photo_ontology).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "photo-schema ontology should be discoverable"
    );

    let metrics_ontology_iri = ib.resource("analytics-engine", "ontology", "metrics-schema");
    let ask_metrics_ontology = format!(
        "ASK {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}Ontology> }}",
        iri = metrics_ontology_iri.as_str()
    );
    let result = projector.query(&ask_metrics_ontology).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "metrics-schema ontology should be discoverable"
    );

    // =========================================================================
    // Step 6c: Verify ALL event stores are discoverable
    // =========================================================================
    let event_store_query = format!(
        "SELECT ?es ?agg WHERE {{ \
         ?es <{RDF_TYPE}> <{PICLOUD_NS}EventStore> ; \
             <{PICLOUD_NS}aggregateType> ?agg \
         }} ORDER BY ?agg"
    );
    let result = projector.query(&event_store_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        2,
        "Should discover 2 event stores (one per aggregate), got: {:?}",
        result.bindings
    );
    assert_eq!(result.bindings[0]["agg"]["value"], "Message");
    assert_eq!(result.bindings[1]["agg"]["value"], "Photo");

    // =========================================================================
    // Step 6d: Verify product events are discoverable
    // =========================================================================
    let product_event_query = format!(
        "SELECT ?e ?eventType WHERE {{ \
         ?e <{RDF_TYPE}> <{PICLOUD_NS}ProductEvent> ; \
            <{PICLOUD_NS}productEventType> ?eventType \
         }} ORDER BY ?eventType"
    );
    let result = projector.query(&product_event_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        2,
        "Should discover 2 product events, got: {:?}",
        result.bindings
    );
    assert_eq!(result.bindings[0]["eventType"]["value"], "MessageSent");
    assert_eq!(result.bindings[1]["eventType"]["value"], "PhotoUploaded");

    // =========================================================================
    // Step 6e: Verify ALL capabilities are discoverable
    // =========================================================================
    let capability_query = format!(
        "SELECT ?c ?name ?input ?output WHERE {{ \
         ?c <{RDF_TYPE}> <{PICLOUD_NS}Capability> ; \
            <{PICLOUD_NS}name> ?name ; \
            <{PICLOUD_NS}inputEvent> ?input ; \
            <{PICLOUD_NS}outputEvent> ?output \
         }} ORDER BY ?name"
    );
    let result = projector.query(&capability_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        2,
        "Should discover 2 capabilities, got: {:?}",
        result.bindings
    );
    assert_eq!(result.bindings[0]["name"]["value"], "image-resize");
    assert_eq!(result.bindings[1]["name"]["value"], "sentiment-analysis");

    // Verify implementor links
    let photo_product_iri = ib.product("photo-app");
    let ask_implementor = format!(
        "ASK {{ <{}> <{PICLOUD_NS}implementedBy> <{}> }}",
        ib.cluster_resource("capabilities", "image-resize").as_str(),
        photo_product_iri.as_str()
    );
    let result = projector.query(&ask_implementor).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "image-resize should be implementedBy photo-app"
    );

    // =========================================================================
    // Step 6f: Verify ALL data products are discoverable
    // =========================================================================
    let data_product_query = format!(
        "SELECT ?dp ?name ?domain WHERE {{ \
         ?dp <{RDF_TYPE}> <{PICLOUD_NS}DataProduct> ; \
             <{PICLOUD_NS}name> ?name ; \
             <{PICLOUD_NS}domain> ?domain \
         }} ORDER BY ?name"
    );
    let result = projector.query(&data_product_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        2,
        "Should discover 2 data products, got: {:?}",
        result.bindings
    );
    assert_eq!(result.bindings[0]["name"]["value"], "chat-metrics");
    assert_eq!(result.bindings[1]["name"]["value"], "photo-catalog");

    // =========================================================================
    // Step 6g: A single union query discovers ALL resource types at once
    // =========================================================================
    // This is the key discoverability assertion: one SPARQL query that returns
    // products, ontologies, event stores, capabilities, and data products.
    let union_query = format!(
        "SELECT ?resource ?kind WHERE {{ \
         {{ ?resource <{PICLOUD_NS}resourceType> \"Product\" . BIND(\"Product\" AS ?kind) }} \
         UNION \
         {{ ?resource <{RDF_TYPE}> <{PICLOUD_NS}Ontology> . BIND(\"Ontology\" AS ?kind) }} \
         UNION \
         {{ ?resource <{RDF_TYPE}> <{PICLOUD_NS}EventStore> . BIND(\"EventStore\" AS ?kind) }} \
         UNION \
         {{ ?resource <{RDF_TYPE}> <{PICLOUD_NS}Capability> . BIND(\"Capability\" AS ?kind) }} \
         UNION \
         {{ ?resource <{RDF_TYPE}> <{PICLOUD_NS}DataProduct> . BIND(\"DataProduct\" AS ?kind) }} \
         }} ORDER BY ?kind ?resource"
    );
    let result = projector.query(&union_query).await.unwrap();

    // Count by kind
    let count_by_kind = |kind: &str| -> usize {
        result
            .bindings
            .iter()
            .filter(|b| b["kind"]["value"] == kind)
            .count()
    };

    assert_eq!(count_by_kind("Product"), 3, "Should find 3 products");
    assert_eq!(count_by_kind("Ontology"), 2, "Should find 2 ontologies");
    assert_eq!(count_by_kind("EventStore"), 2, "Should find 2 event stores");
    assert_eq!(count_by_kind("Capability"), 2, "Should find 2 capabilities");
    assert_eq!(
        count_by_kind("DataProduct"),
        2,
        "Should find 2 data products"
    );

    // Total: 3 + 2 + 2 + 2 + 2 = 11 discoverable resources
    assert_eq!(
        result.bindings.len(),
        11,
        "Union query should return all 11 discoverable resources, got: {:?}",
        result.bindings
    );

    // =========================================================================
    // Step 6h: Verify product-scoped resources also appear in named graphs
    // =========================================================================
    // Ontology in photo-app's named graph
    let photo_graph = ib.product_graph("photo-app");
    let graph_query = format!(
        "ASK {{ GRAPH <{graph}> {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}Ontology> }} }}",
        graph = photo_graph.as_str(),
        iri = photo_ontology_iri.as_str()
    );
    let result = projector.query(&graph_query).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "photo-schema should appear in photo-app's named graph"
    );

    // Event store in chat-app's named graph
    let chat_graph = ib.product_graph("chat-app");
    let chat_es_iri = ib.resource("chat-app", "event-store", "chat-events");
    let graph_query = format!(
        "ASK {{ GRAPH <{graph}> {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}EventStore> }} }}",
        graph = chat_graph.as_str(),
        iri = chat_es_iri.as_str()
    );
    let result = projector.query(&graph_query).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "chat-events event store should appear in chat-app's named graph"
    );

    // =========================================================================
    // Step 6i: Cross-product discovery of product events by aggregate type
    // =========================================================================
    let cross_events_query = format!(
        "SELECT ?e ?product ?aggType WHERE {{ \
         ?e <{RDF_TYPE}> <{PICLOUD_NS}ProductEvent> ; \
            <{PICLOUD_NS}aggregateType> ?aggType ; \
            <{PICLOUD_NS}product> ?product \
         }} ORDER BY ?product"
    );
    let result = projector.query(&cross_events_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        2,
        "Should discover product events from both products"
    );
}

// ============================================================================
// TC-341 — Discoverability exit — SPARQL returns all products and ontologies
// ============================================================================
/// Exit criteria for FT-085:
///
/// Verifies the minimum discoverability guarantee: a cluster-wide SPARQL query
/// returns all products and their associated ontologies, capabilities, event
/// stores, and data products without requiring knowledge of which products exist.
///
/// This is the "can I discover what's running on this cluster?" question. A new
/// product deployed after initial setup must be immediately discoverable. An
/// ontology loaded for any product must appear in both the default graph and the
/// product's named graph.
///
/// Steps:
///   1. Deploy 3 products with distinct versions
///   2. Declare ontologies for each product
///   3. Declare a capability, an event store, and a data product
///   4. Verify a single SPARQL query discovers all products with versions
///   5. Verify a single SPARQL query discovers all ontologies with product links
///   6. Verify cluster-wide resource count matches expectations
///   7. Deploy a 4th product after initial setup — verify it's immediately discoverable
#[tokio::test]
async fn tc341_discoverability_exit_sparql_returns_all_products_and_ontologies() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // =========================================================================
    // Step 1: Deploy 3 products
    // =========================================================================
    projector
        .project(&make_product_deployed("web-frontend", "1.2.0"))
        .await
        .unwrap();
    projector
        .project(&make_product_deployed("api-backend", "3.0.1"))
        .await
        .unwrap();
    projector
        .project(&make_product_deployed("ml-service", "0.9.0"))
        .await
        .unwrap();

    // =========================================================================
    // Step 2: Declare ontologies for each product
    // =========================================================================
    projector
        .project(&make_ontology_declared(
            "web-frontend",
            "ui-schema",
            "ontologies/ui-schema.ttl",
            "turtle",
            "1.2.0",
        ))
        .await
        .unwrap();
    projector
        .project(&make_ontology_declared(
            "api-backend",
            "api-schema",
            "ontologies/api-schema.ttl",
            "turtle",
            "3.0.1",
        ))
        .await
        .unwrap();
    projector
        .project(&make_ontology_declared(
            "ml-service",
            "model-schema",
            "ontologies/model-schema.shacl",
            "shacl",
            "0.9.0",
        ))
        .await
        .unwrap();

    // =========================================================================
    // Step 3: Declare a capability, event store, and data product
    // =========================================================================
    projector
        .project(&make_capability_declared(
            "inference",
            "1.0.0",
            "PredictionRequested",
            "PredictionCompleted",
        ))
        .await
        .unwrap();
    projector
        .project(&make_event_store_declared(
            "api-backend",
            "api-events",
            serde_json::json!([
                {"aggregate_type": "Request", "schema_file": "schemas/request.json"}
            ]),
        ))
        .await
        .unwrap();
    projector
        .project(&make_data_domain_declared(
            "ml-domain",
            "https://picloud.local/identities/steward-ml",
            "internal",
        ))
        .await
        .unwrap();
    projector
        .project(&make_data_product_declared(
            "ml-service",
            "model-predictions",
            "ml-domain",
            "1.0.0",
            Some("PT5M"),
        ))
        .await
        .unwrap();

    // =========================================================================
    // Step 4: Verify all products are discoverable with versions
    // =========================================================================
    let product_query = format!(
        "SELECT ?p ?name ?version WHERE {{ \
         ?p <{PICLOUD_NS}resourceType> \"Product\" ; \
            <{PICLOUD_NS}name> ?name ; \
            <{PICLOUD_NS}version> ?version \
         }} ORDER BY ?name"
    );
    let result = projector.query(&product_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        3,
        "Should discover all 3 products"
    );
    assert_eq!(result.bindings[0]["name"]["value"], "api-backend");
    assert_eq!(result.bindings[0]["version"]["value"], "3.0.1");
    assert_eq!(result.bindings[1]["name"]["value"], "ml-service");
    assert_eq!(result.bindings[1]["version"]["value"], "0.9.0");
    assert_eq!(result.bindings[2]["name"]["value"], "web-frontend");
    assert_eq!(result.bindings[2]["version"]["value"], "1.2.0");

    // =========================================================================
    // Step 5: Verify all ontologies are discoverable with product links
    // =========================================================================
    let ontology_query = format!(
        "SELECT ?o ?product ?format WHERE {{ \
         ?o <{RDF_TYPE}> <{PICLOUD_NS}Ontology> ; \
            <{PICLOUD_NS}product> ?product . \
         OPTIONAL {{ ?o <{PICLOUD_NS}format> ?format }} \
         }} ORDER BY ?product"
    );
    let result = projector.query(&ontology_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        3,
        "Should discover all 3 ontologies with product links, got: {:?}",
        result.bindings
    );
    assert_eq!(result.bindings[0]["product"]["value"], "api-backend");
    assert_eq!(result.bindings[1]["product"]["value"], "ml-service");
    assert_eq!(result.bindings[2]["product"]["value"], "web-frontend");

    // Verify ontology format details are present
    let shacl_query = format!(
        "SELECT ?o WHERE {{ \
         ?o <{RDF_TYPE}> <{PICLOUD_NS}Ontology> ; \
            <{PICLOUD_NS}format> \"shacl\" \
         }}"
    );
    let result = projector.query(&shacl_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        1,
        "Should find exactly 1 SHACL ontology"
    );

    // =========================================================================
    // Step 6: Verify cluster-wide resource count
    // =========================================================================
    // Count all distinct resource kinds present in the cluster
    let kind_count_query = format!(
        "SELECT ?kind (COUNT(DISTINCT ?r) AS ?count) WHERE {{ \
         {{ ?r <{PICLOUD_NS}resourceType> \"Product\" . BIND(\"Product\" AS ?kind) }} \
         UNION \
         {{ ?r <{RDF_TYPE}> <{PICLOUD_NS}Ontology> . BIND(\"Ontology\" AS ?kind) }} \
         UNION \
         {{ ?r <{RDF_TYPE}> <{PICLOUD_NS}EventStore> . BIND(\"EventStore\" AS ?kind) }} \
         UNION \
         {{ ?r <{RDF_TYPE}> <{PICLOUD_NS}Capability> . BIND(\"Capability\" AS ?kind) }} \
         UNION \
         {{ ?r <{RDF_TYPE}> <{PICLOUD_NS}DataProduct> . BIND(\"DataProduct\" AS ?kind) }} \
         }} GROUP BY ?kind ORDER BY ?kind"
    );
    let result = projector.query(&kind_count_query).await.unwrap();
    // We should have 5 different kinds
    assert_eq!(
        result.bindings.len(),
        5,
        "Should have 5 resource kinds discoverable, got: {:?}",
        result.bindings
    );

    // =========================================================================
    // Step 7: Deploy a 4th product and verify immediate discoverability
    // =========================================================================
    projector
        .project(&make_product_deployed("monitoring-agent", "1.0.0"))
        .await
        .unwrap();
    projector
        .project(&make_ontology_declared(
            "monitoring-agent",
            "alerts-schema",
            "ontologies/alerts-schema.ttl",
            "turtle",
            "1.0.0",
        ))
        .await
        .unwrap();

    // Re-run the product discovery query
    let result = projector.query(&product_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        4,
        "Should now discover 4 products after deploying monitoring-agent"
    );
    // Verify the new product is in the result set
    let names: Vec<&str> = result
        .bindings
        .iter()
        .filter_map(|b| b["name"]["value"].as_str())
        .collect();
    assert!(
        names.contains(&"monitoring-agent"),
        "monitoring-agent should be immediately discoverable, got names: {:?}",
        names
    );

    // Re-run the ontology discovery query
    let ontology_query_all = format!(
        "SELECT ?o ?product WHERE {{ \
         ?o <{RDF_TYPE}> <{PICLOUD_NS}Ontology> ; \
            <{PICLOUD_NS}product> ?product \
         }} ORDER BY ?product"
    );
    let result = projector.query(&ontology_query_all).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        4,
        "Should now discover 4 ontologies after adding monitoring-agent's ontology"
    );

    // Verify the new ontology is in the named graph
    let monitor_graph = ib.product_graph("monitoring-agent");
    let monitor_ontology_iri = ib.resource("monitoring-agent", "ontology", "alerts-schema");
    let graph_query = format!(
        "ASK {{ GRAPH <{graph}> {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}Ontology> }} }}",
        graph = monitor_graph.as_str(),
        iri = monitor_ontology_iri.as_str()
    );
    let result = projector.query(&graph_query).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "alerts-schema should appear in monitoring-agent's named graph"
    );

    // Verify that the cluster union query now shows 12 total resources (4P + 4O + 1ES + 1C + 1DP + 1DD = but we only count 5 types)
    let union_query = format!(
        "SELECT ?resource ?kind WHERE {{ \
         {{ ?resource <{PICLOUD_NS}resourceType> \"Product\" . BIND(\"Product\" AS ?kind) }} \
         UNION \
         {{ ?resource <{RDF_TYPE}> <{PICLOUD_NS}Ontology> . BIND(\"Ontology\" AS ?kind) }} \
         UNION \
         {{ ?resource <{RDF_TYPE}> <{PICLOUD_NS}EventStore> . BIND(\"EventStore\" AS ?kind) }} \
         UNION \
         {{ ?resource <{RDF_TYPE}> <{PICLOUD_NS}Capability> . BIND(\"Capability\" AS ?kind) }} \
         UNION \
         {{ ?resource <{RDF_TYPE}> <{PICLOUD_NS}DataProduct> . BIND(\"DataProduct\" AS ?kind) }} \
         }}"
    );
    let result = projector.query(&union_query).await.unwrap();
    // 4 products + 4 ontologies + 1 event store + 1 capability + 1 data product = 11
    assert_eq!(
        result.bindings.len(),
        11,
        "Final union query should discover all 11 resources, got {}",
        result.bindings.len()
    );
}
