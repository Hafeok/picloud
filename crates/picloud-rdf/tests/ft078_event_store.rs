/// FT-078 Integration Tests — Event Store Resource Type
///
/// Covers:
///   TC-230: Product appends to event store and queries RDF projection (exit-criteria)
///
/// Verifies that a product can:
///   1. Declare an event-store resource with aggregate definitions
///   2. Append events to the product event store
///   3. Have those events automatically projected into the product's RDF graph
///   4. Query the projected state via SPARQL

use std::sync::Arc;

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{EventLog, StateProjector};
use picloud_events::InMemoryEventLog;
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
// TC-230 — Product appends to event store and queries RDF projection
// ============================================================================
/// Exit criteria: a product declares an event store with aggregate definitions,
/// appends events to its product event store, the platform projects those events
/// into the product's RDF named graph, and SPARQL queries return the projected
/// state correctly.
#[tokio::test]
async fn tc230_product_appends_to_event_store_and_queries_rdf_projection() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();
    let event_log = Arc::new(InMemoryEventLog::new());

    let product_name = "photo-app";
    let store_name = "photos";

    // --- Step 1: Deploy the product ---
    projector
        .project(&make_product_deployed(product_name, "1.0.0"))
        .await
        .unwrap();

    // --- Step 2: Declare an EventStore resource with aggregates ---
    let aggregates = serde_json::json!([
        { "aggregate_type": "Photo", "schema_file": "schemas/photo-events.ttl" },
        { "aggregate_type": "Album", "schema_file": "schemas/album-events.ttl" }
    ]);
    let declare_event = make_event_store_declared(product_name, store_name, aggregates);
    projector.project(&declare_event).await.unwrap();

    // Mark it ready
    projector
        .project(&make_resource_ready(product_name, "event-store", store_name))
        .await
        .unwrap();

    // --- Step 3: Verify EventStore is typed correctly in default graph ---
    let store_iri = ib.resource(product_name, "event-store", store_name);
    let ask = format!(
        "ASK {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}EventStore> }}",
        iri = store_iri.as_str()
    );
    let r = projector.query(&ask).await.unwrap();
    assert_eq!(
        r.bindings[0]["result"], true,
        "EventStore resource should be typed as picloud:EventStore"
    );

    // --- Step 4: Verify aggregate types are projected ---
    let q = format!(
        "SELECT ?agg WHERE {{ <{iri}> <{PICLOUD_NS}aggregateType> ?agg }} ORDER BY ?agg",
        iri = store_iri.as_str()
    );
    let r = projector.query(&q).await.unwrap();
    assert_eq!(
        r.bindings.len(),
        2,
        "Should have 2 aggregate types, got: {:?}",
        r.bindings
    );
    let agg_types: Vec<&str> = r
        .bindings
        .iter()
        .filter_map(|b| b["agg"]["value"].as_str())
        .collect();
    assert!(
        agg_types.contains(&"Album"),
        "Should contain Album aggregate type"
    );
    assert!(
        agg_types.contains(&"Photo"),
        "Should contain Photo aggregate type"
    );

    // --- Step 5: Verify EventStore is Ready ---
    let ask = format!(
        "ASK {{ <{iri}> <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> }}",
        iri = store_iri.as_str()
    );
    let r = projector.query(&ask).await.unwrap();
    assert_eq!(
        r.bindings[0]["result"], true,
        "EventStore should be in Ready state"
    );

    // --- Step 6: Verify EventStore appears in product's named graph ---
    let product_graph = ib.product_graph(product_name);
    let ask = format!(
        "ASK {{ GRAPH <{graph}> {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}EventStore> }} }}",
        graph = product_graph.as_str(),
        iri = store_iri.as_str()
    );
    let r = projector.query(&ask).await.unwrap();
    assert_eq!(
        r.bindings[0]["result"], true,
        "EventStore should appear in product's named graph"
    );

    // --- Step 7: Append events to the product event store ---
    // The ProductEventStore wraps the event log and enforces product scope
    let product_store =
        picloud_events::ProductEventStore::new(product_name, event_log.clone());

    // Create PhotoUploaded events
    let photo1_event = make_product_event_appended(
        product_name,
        store_name,
        "Photo",
        "photo-001",
        "PhotoUploaded",
        serde_json::json!({
            "title": "Sunset at the Beach",
            "width": "1920",
            "height": "1080",
        }),
    );
    let photo2_event = make_product_event_appended(
        product_name,
        store_name,
        "Photo",
        "photo-002",
        "PhotoUploaded",
        serde_json::json!({
            "title": "Mountain View",
            "width": "3840",
            "height": "2160",
        }),
    );

    // Create AlbumCreated event
    let album_event = make_product_event_appended(
        product_name,
        store_name,
        "Album",
        "album-001",
        "AlbumCreated",
        serde_json::json!({
            "title": "Vacation 2026",
            "photoCount": "2",
        }),
    );

    // Append to product event store (enforces product scope)
    product_store
        .append(photo1_event.clone())
        .await
        .unwrap();
    product_store
        .append(photo2_event.clone())
        .await
        .unwrap();
    product_store
        .append(album_event.clone())
        .await
        .unwrap();

    // Verify product event store scoping: events are product-scoped
    let product_events = product_store.events_since(0).await;
    assert_eq!(
        product_events.len(),
        3,
        "Product event store should have 3 events"
    );
    for e in &product_events {
        assert_eq!(
            e.product.as_deref(),
            Some(product_name),
            "All events should be scoped to product"
        );
    }

    // --- Step 8: Project the appended events into RDF ---
    // In the real system, the platform event loop does this automatically.
    // Here we simulate it by projecting each event through the projector.
    for e in &product_events {
        projector.project(e).await.unwrap();
    }

    // --- Step 9: Query the RDF projection for Photo events ---
    let q = format!(
        r#"SELECT ?event ?title WHERE {{
            ?event <{PICLOUD_NS}aggregateType> "Photo" .
            ?event <{PICLOUD_NS}productEventType> "PhotoUploaded" .
            ?event <{PICLOUD_NS}title> ?title .
        }} ORDER BY ?title"#
    );
    let r = projector.query(&q).await.unwrap();
    assert_eq!(
        r.bindings.len(),
        2,
        "Should find 2 PhotoUploaded events, got: {:?}",
        r.bindings
    );
    let titles: Vec<&str> = r
        .bindings
        .iter()
        .filter_map(|b| b["title"]["value"].as_str())
        .collect();
    assert!(
        titles.contains(&"Mountain View"),
        "Should contain 'Mountain View'"
    );
    assert!(
        titles.contains(&"Sunset at the Beach"),
        "Should contain 'Sunset at the Beach'"
    );

    // --- Step 10: Query the RDF projection for Album events ---
    let q = format!(
        r#"SELECT ?event ?title ?count WHERE {{
            ?event <{PICLOUD_NS}aggregateType> "Album" .
            ?event <{PICLOUD_NS}productEventType> "AlbumCreated" .
            ?event <{PICLOUD_NS}title> ?title .
            ?event <{PICLOUD_NS}photoCount> ?count .
        }}"#
    );
    let r = projector.query(&q).await.unwrap();
    assert_eq!(
        r.bindings.len(),
        1,
        "Should find 1 AlbumCreated event, got: {:?}",
        r.bindings
    );
    assert_eq!(
        r.bindings[0]["title"]["value"].as_str().unwrap(),
        "Vacation 2026",
        "Album title should be 'Vacation 2026'"
    );
    assert_eq!(
        r.bindings[0]["count"]["value"].as_str().unwrap(),
        "2",
        "Album photo count should be 2"
    );

    // --- Step 11: Query the product named graph specifically ---
    let q = format!(
        r#"SELECT ?event ?type WHERE {{
            GRAPH <{graph}> {{
                ?event <{RDF_TYPE}> <{PICLOUD_NS}ProductEvent> .
                ?event <{PICLOUD_NS}productEventType> ?type .
            }}
        }} ORDER BY ?type"#,
        graph = product_graph.as_str()
    );
    let r = projector.query(&q).await.unwrap();
    assert_eq!(
        r.bindings.len(),
        3,
        "Product named graph should contain 3 ProductEvent resources, got: {:?}",
        r.bindings
    );
    let event_types: Vec<&str> = r
        .bindings
        .iter()
        .filter_map(|b| b["type"]["value"].as_str())
        .collect();
    assert!(
        event_types.contains(&"PhotoUploaded"),
        "Should contain PhotoUploaded events"
    );
    assert!(
        event_types.contains(&"AlbumCreated"),
        "Should contain AlbumCreated events"
    );

    // --- Step 12: Verify product isolation ---
    // Another product should see nothing in its named graph
    projector
        .project(&make_product_deployed("chat-app", "1.0.0"))
        .await
        .unwrap();
    let chat_graph = ib.product_graph("chat-app");
    let q = format!(
        r#"SELECT (COUNT(?event) AS ?count) WHERE {{
            GRAPH <{graph}> {{
                ?event <{RDF_TYPE}> <{PICLOUD_NS}ProductEvent> .
            }}
        }}"#,
        graph = chat_graph.as_str()
    );
    let r = projector.query(&q).await.unwrap();
    let count: i64 = r.bindings[0]["count"]["value"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        count, 0,
        "chat-app should have no product events — isolated from photo-app"
    );

    // --- Step 13: Verify SPARQL ASK query works against projected events ---
    let q = format!(
        r#"ASK {{
            ?event <{PICLOUD_NS}aggregateType> "Photo" .
            ?event <{PICLOUD_NS}title> "Sunset at the Beach" .
        }}"#
    );
    let r = projector.query(&q).await.unwrap();
    assert_eq!(
        r.bindings[0]["result"], true,
        "ASK query should confirm Sunset at the Beach photo exists"
    );

    // --- Step 14: Count all product events via aggregate type ---
    let q = format!(
        r#"SELECT ?aggType (COUNT(?event) AS ?count) WHERE {{
            ?event <{RDF_TYPE}> <{PICLOUD_NS}ProductEvent> .
            ?event <{PICLOUD_NS}aggregateType> ?aggType .
            ?event <{PICLOUD_NS}product> "{product_name}" .
        }} GROUP BY ?aggType ORDER BY ?aggType"#
    );
    let r = projector.query(&q).await.unwrap();
    assert_eq!(
        r.bindings.len(),
        2,
        "Should have 2 aggregate type groups"
    );
    // Find Album count
    let album_row = r
        .bindings
        .iter()
        .find(|b| b["aggType"]["value"].as_str() == Some("Album"))
        .expect("Should have Album aggregate type");
    assert_eq!(
        album_row["count"]["value"].as_str().unwrap(),
        "1",
        "Album aggregate should have 1 event"
    );
    // Find Photo count
    let photo_row = r
        .bindings
        .iter()
        .find(|b| b["aggType"]["value"].as_str() == Some("Photo"))
        .expect("Should have Photo aggregate type");
    assert_eq!(
        photo_row["count"]["value"].as_str().unwrap(),
        "2",
        "Photo aggregate should have 2 events"
    );
}
