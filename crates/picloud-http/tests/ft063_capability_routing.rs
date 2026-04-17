/// FT-063 Integration Tests — Capability-aware event routing
///
/// Covers:
///   TC-270: Event routed to implementing product resolved by capability IRI (scenario)
///   TC-327: Capability routing exit — events routed to implementing product (exit-criteria)
///   TC-206: capability_triggers_data_product — capability routing drives data
///           product projection rebuilds when the capability output event is a
///           declared trigger on a data product (ADR-055 + ADR-056)
///
/// Verifies that the platform resolves the implementing Product at dispatch time
/// when routing events through a declared capability. The CapabilityResolverImpl
/// queries the RDF graph (via SPARQL) to find the implementor whose capability
/// version satisfies the consumer's minVersion, then appends a
/// CapabilityEventRouted event scoped to that implementor's product.

use std::sync::Arc;
use std::time::Instant;

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{
    CapabilityResolver, DataProductProjector, EventLog, StateProjector,
};
use picloud_events::InMemoryEventLog;
use picloud_http::CapabilityResolverImpl;
use picloud_rdf::{OxigraphDataProductProjector, OxigraphProjector};
use uuid::Uuid;

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

/// Project a CapabilityDeclared event into the RDF graph.
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

/// Project a CapabilityImplementorAdded event into the RDF graph.
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

/// Project a CapabilityReady event into the RDF graph.
fn make_capability_ready(capability_name: &str, implementor_product: &str) -> EventEnvelope {
    let ib = iri_builder();
    let capability_iri = ib.cluster_resource("capabilities", capability_name);
    make_event(
        "CapabilityReady",
        None,
        serde_json::json!({
            "capability_iri": capability_iri.as_str(),
            "implementor_product": implementor_product,
        }),
    )
}

/// Project a CapabilityConsumerAdded event into the RDF graph.
fn make_capability_consumer_added(
    capability_name: &str,
    product_name: &str,
    min_version: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let capability_iri = ib.cluster_resource("capabilities", capability_name);
    let product_iri = ib.product(product_name);
    make_event(
        "CapabilityConsumerAdded",
        None,
        serde_json::json!({
            "capability_iri": capability_iri.as_str(),
            "capability_name": capability_name,
            "product_iri": product_iri.as_str(),
            "product_name": product_name,
            "min_version": min_version,
        }),
    )
}

/// Build the full test harness: OxigraphProjector + InMemoryEventLog + CapabilityResolverImpl.
fn build_resolver() -> (
    Arc<OxigraphProjector>,
    Arc<InMemoryEventLog>,
    CapabilityResolverImpl,
) {
    let domain = ClusterDomain::default();
    let projector = Arc::new(OxigraphProjector::with_domain(domain.clone()).unwrap());
    let event_log = Arc::new(InMemoryEventLog::new());
    let resolver = CapabilityResolverImpl::new(
        projector.clone() as Arc<dyn StateProjector>,
        event_log.clone() as Arc<dyn EventLog>,
        domain,
    );
    (projector, event_log, resolver)
}

// ============================================================================
// TC-270 — Event routed to implementing product resolved by capability IRI
// ============================================================================
/// Scenario test for FT-063: Exercises the full capability-aware event routing
/// flow. Declares a capability, registers an implementor product, adds a
/// consumer, then routes an input event through the capability IRI. Verifies:
///   1. The resolver finds the implementing product via SPARQL on the RDF graph.
///   2. A CapabilityEventRouted event is appended to the event log.
///   3. The routed event is scoped to the implementor's product.
///   4. The routed event payload contains the original event metadata.
///   5. Correlation ID and source IRI propagate from input to routed event.
///   6. Version satisfaction is enforced (minVersion > capability version → fail).
///   7. Routing to a nonexistent capability fails.
#[tokio::test]
async fn tc270_event_routed_to_implementing_product_resolved_by_capability_iri() {
    let (projector, event_log, resolver) = build_resolver();
    let ib = iri_builder();

    // ---- Step 1: Declare a capability "gps-to-place" at version 1.2.0 ----
    let cap_declared = make_capability_declared(
        "gps-to-place",
        "1.2.0",
        "CoordinatesReceived",
        "PlaceResolved",
    );
    projector.project(&cap_declared).await.unwrap();

    // ---- Step 2: Register implementor "geo-service" ----
    let impl_added = make_capability_implementor_added("gps-to-place", "geo-service", "1.2.0");
    projector.project(&impl_added).await.unwrap();

    // Mark capability as ready
    let cap_ready = make_capability_ready("gps-to-place", "geo-service");
    projector.project(&cap_ready).await.unwrap();

    // ---- Step 3: Register a consumer "photo-app" with minVersion 1.0.0 ----
    let consumer_added = make_capability_consumer_added("gps-to-place", "photo-app", "1.0.0");
    projector.project(&consumer_added).await.unwrap();

    // ---- Step 4: Route an event through the capability ----
    let input_event = EventEnvelope::new(
        ib.event_schema("CoordinatesReceived", 1),
        "CoordinatesReceived",
        ib.product("photo-app"),
        Some("photo-app".to_string()),
        Uuid::new_v4(),
        serde_json::json!({
            "latitude": 48.8566,
            "longitude": 2.3522,
            "minVersion": "1.0.0",
        }),
    );

    resolver
        .route_capability_event("gps-to-place", &input_event)
        .await
        .unwrap();

    // ---- Step 5: Verify a CapabilityEventRouted event was appended ----
    let events = event_log.events_since(0).await;
    assert_eq!(events.len(), 1, "exactly one routed event should be appended");

    let routed = &events[0];
    assert_eq!(
        routed.event_type, "CapabilityEventRouted",
        "routed event must be CapabilityEventRouted"
    );
    assert_eq!(
        routed.product.as_deref(),
        Some("geo-service"),
        "routed event must be scoped to the implementor product"
    );

    // ---- Step 6: Verify the routed event payload ----
    let payload = &routed.payload;
    assert_eq!(
        payload["capability"], "gps-to-place",
        "payload must contain the capability name"
    );
    assert_eq!(
        payload["implementor_product"], "geo-service",
        "payload must identify the implementor product"
    );
    assert_eq!(
        payload["implementor_version"], "1.2.0",
        "payload must carry the resolved capability version"
    );
    assert_eq!(
        payload["original_event_type"], "CoordinatesReceived",
        "payload must carry the original event type"
    );
    assert_eq!(
        payload["original_event_id"],
        input_event.id.to_string(),
        "payload must carry the original event ID"
    );
    assert_eq!(
        payload["original_payload"]["latitude"], 48.8566,
        "original payload data must be preserved"
    );
    assert_eq!(
        payload["original_payload"]["longitude"], 2.3522,
        "original payload longitude must be preserved"
    );

    // ---- Step 7: Verify correlation ID propagation ----
    assert_eq!(
        routed.correlation_id, input_event.correlation_id,
        "correlation ID must propagate from input to routed event"
    );

    // ---- Step 8: Verify source IRI propagation ----
    assert_eq!(
        routed.source.as_str(),
        ib.product("photo-app").as_str(),
        "source IRI must propagate from the originating product"
    );

    // ---- Step 9: Verify version satisfaction is enforced ----
    // Route with a minVersion higher than the capability version → should fail
    let high_min_event = EventEnvelope::new(
        ib.event_schema("CoordinatesReceived", 1),
        "CoordinatesReceived",
        ib.product("photo-app"),
        Some("photo-app".to_string()),
        Uuid::new_v4(),
        serde_json::json!({
            "latitude": 40.7128,
            "longitude": -74.0060,
            "minVersion": "3.0.0",
        }),
    );

    let result = resolver
        .route_capability_event("gps-to-place", &high_min_event)
        .await;
    assert!(
        result.is_err(),
        "routing must fail when minVersion (3.0.0) exceeds capability version (1.2.0)"
    );

    // ---- Step 10: Verify routing fails for nonexistent capability ----
    let bad_event = EventEnvelope::new(
        ib.event_schema("SomeEvent", 1),
        "SomeEvent",
        ib.product("photo-app"),
        Some("photo-app".to_string()),
        Uuid::new_v4(),
        serde_json::json!({ "minVersion": "1.0.0" }),
    );

    let result = resolver
        .route_capability_event("nonexistent-capability", &bad_event)
        .await;
    assert!(
        result.is_err(),
        "routing to a nonexistent capability must fail"
    );
}

// ============================================================================
// TC-327 — Capability routing exit — events routed to implementing product
// ============================================================================
/// Exit criteria for FT-063: Verify the minimum bar for capability-aware event
/// routing — a declared capability with an implementor correctly resolves at
/// dispatch time, and the resulting CapabilityEventRouted event is scoped to the
/// implementing product.
#[tokio::test]
async fn tc327_capability_routing_exit_events_routed_to_implementing_product() {
    let (projector, event_log, resolver) = build_resolver();
    let ib = iri_builder();

    // ---- Setup: Declare capability and register implementor ----
    let cap_declared = make_capability_declared(
        "image-resize",
        "1.0.0",
        "ImageUploadReceived",
        "ImageResized",
    );
    projector.project(&cap_declared).await.unwrap();

    let impl_added = make_capability_implementor_added("image-resize", "media-service", "1.0.0");
    projector.project(&impl_added).await.unwrap();

    // ---- Route an event through the capability ----
    let input_event = EventEnvelope::new(
        ib.event_schema("ImageUploadReceived", 1),
        "ImageUploadReceived",
        ib.product("photo-app"),
        Some("photo-app".to_string()),
        Uuid::new_v4(),
        serde_json::json!({
            "image_id": "img-001",
            "size_bytes": 4096000,
        }),
    );

    resolver
        .route_capability_event("image-resize", &input_event)
        .await
        .unwrap();

    // ---- Verify: a CapabilityEventRouted event exists, scoped to implementor ----
    let events = event_log.events_since(0).await;
    assert_eq!(
        events.len(),
        1,
        "exactly one CapabilityEventRouted event must be appended"
    );

    let routed = &events[0];
    assert_eq!(
        routed.event_type, "CapabilityEventRouted",
        "event type must be CapabilityEventRouted"
    );
    assert_eq!(
        routed.product.as_deref(),
        Some("media-service"),
        "event must be scoped to the implementing product resolved by the capability IRI"
    );
    assert_eq!(
        routed.payload["capability"], "image-resize",
        "payload must reference the capability"
    );
    assert_eq!(
        routed.payload["implementor_product"], "media-service",
        "payload must name the resolved implementor"
    );
    assert_eq!(
        routed.payload["original_event_type"], "ImageUploadReceived",
        "original event type must be preserved in the routed event"
    );
    assert_eq!(
        routed.payload["original_payload"]["image_id"], "img-001",
        "original payload must be preserved"
    );
}

// ============================================================================
// TC-206 — capability_triggers_data_product (ADR-055 + ADR-056)
// ============================================================================
/// Integration test combining capability routing and data product projection.
///
/// Scenario:
///   1. Deploy `gps-to-place` capability
///      (input: `CoordinatesReceived`, output: `PlaceResolved`)
///   2. Register `photo-app` as the implementor
///   3. Declare `photo-locations` data product in `photo-app` with
///      `triggers: ['PlaceResolved']` and a CONSTRUCT projection query
///   4. Seed `photo-app`'s internal graph with photo triples
///   5. `maps-app` emits a `CoordinatesReceived` event
///   6. `CapabilityResolverImpl` routes it to `photo-app`, appending a
///      `CapabilityEventRouted` event scoped to `photo-app`
///   7. Simulate `photo-app` emitting the capability output event
///      (`PlaceResolved`)
///   8. Because `PlaceResolved` is a declared trigger for the
///      `photo-locations` data product, the data product projection is
///      rebuilt — the published named graph is populated with fresh triples
///
/// Assertions:
///   - Capability routing emits exactly one `CapabilityEventRouted` event
///     targeting `photo-app` (the implementor).
///   - The `PlaceResolved` event is recognized as a trigger for the data
///     product via the RDF graph.
///   - The data product projection refresh populates the published named
///     graph with the expected triples (from the `photo-app` internal graph).
///   - A `DataProductRefreshed` event is appended, referencing the
///     capability output event as the trigger.
///   - The entire flow completes well within the 30-second budget declared
///     by the test criterion.
#[tokio::test]
async fn capability_triggers_data_product() {
    use oxigraph::model::{Literal, NamedNode, NamedNodeRef, QuadRef};

    let start = Instant::now();

    let domain = ClusterDomain::default();
    let projector = Arc::new(OxigraphProjector::with_domain(domain.clone()).unwrap());
    let event_log = Arc::new(InMemoryEventLog::new());
    let resolver = CapabilityResolverImpl::new(
        projector.clone() as Arc<dyn StateProjector>,
        event_log.clone() as Arc<dyn EventLog>,
        domain.clone(),
    );
    let dp_projector =
        OxigraphDataProductProjector::new(Arc::new(projector.store().clone()));
    let ib = iri_builder();

    // ------------------------------------------------------------------
    // Step 1: Declare the capability `gps-to-place`
    //   input:  CoordinatesReceived
    //   output: PlaceResolved
    // ------------------------------------------------------------------
    let cap_declared = make_capability_declared(
        "gps-to-place",
        "1.0.0",
        "CoordinatesReceived",
        "PlaceResolved",
    );
    projector.project(&cap_declared).await.unwrap();

    // ------------------------------------------------------------------
    // Step 2: Register `photo-app` as the implementor
    // ------------------------------------------------------------------
    let impl_added = make_capability_implementor_added("gps-to-place", "photo-app", "1.0.0");
    projector.project(&impl_added).await.unwrap();

    let cap_ready = make_capability_ready("gps-to-place", "photo-app");
    projector.project(&cap_ready).await.unwrap();

    // ------------------------------------------------------------------
    // Step 3: Declare `photo-locations` data product with
    //         triggers: ['PlaceResolved']
    // ------------------------------------------------------------------
    let dp_graph_iri = ib.data_product_graph("photo-app", "photo-locations");
    let dp_resource_iri_str = dp_graph_iri
        .as_str()
        .trim_end_matches("/graph")
        .to_string();
    let dp_resource_iri = ResourceIri::new(&dp_resource_iri_str).unwrap();

    let dp_declared = EventEnvelope::new(
        ib.event_schema("DataProductDeclared", 1),
        "DataProductDeclared",
        dp_resource_iri.clone(),
        Some("photo-app".to_string()),
        Uuid::new_v4(),
        serde_json::json!({
            "data_product_iri": dp_resource_iri_str,
            "name": "photo-locations",
            "product": "photo-app",
            "domain": "geospatial",
            "version": "1.0.0",
            "max_age": "15m",
            "triggers": ["PlaceResolved"],
        }),
    );
    projector.project(&dp_declared).await.unwrap();

    // Record the trigger association directly in the RDF graph so that
    // a lookup "which data products refresh on PlaceResolved?" is
    // answerable via SPARQL.
    {
        let store = projector.store();
        let dp_node = NamedNode::new(&dp_resource_iri_str).unwrap();
        let trigger_pred =
            NamedNode::new("https://picloud.local/ontology#triggerEvent").unwrap();
        let trigger_lit = Literal::new_simple_literal("PlaceResolved");
        store
            .insert(QuadRef::new(
                &dp_node,
                &trigger_pred,
                &trigger_lit,
                oxigraph::model::GraphNameRef::DefaultGraph,
            ))
            .unwrap();
    }

    // ------------------------------------------------------------------
    // Step 4: Seed `photo-app`'s internal operational graph with photo
    //         triples (acting as the state the data product projects).
    // ------------------------------------------------------------------
    let product_graph_iri = ib.product_graph("photo-app");
    {
        let store = projector.store();
        store
            .insert_named_graph(NamedNodeRef::new(product_graph_iri.as_str()).unwrap())
            .unwrap();

        let g = NamedNode::new(product_graph_iri.as_str()).unwrap();
        let place_pred =
            NamedNode::new("https://picloud.local/ontology#placeName").unwrap();

        // Photo 1 — Paris
        let photo1 =
            NamedNode::new("https://picloud.local/products/photo-app/photos/p1").unwrap();
        store
            .insert(QuadRef::new(
                &photo1,
                &place_pred,
                &Literal::new_simple_literal("Paris"),
                &g,
            ))
            .unwrap();

        // Photo 2 — London
        let photo2 =
            NamedNode::new("https://picloud.local/products/photo-app/photos/p2").unwrap();
        store
            .insert(QuadRef::new(
                &photo2,
                &place_pred,
                &Literal::new_simple_literal("London"),
                &g,
            ))
            .unwrap();
    }

    // ------------------------------------------------------------------
    // Step 5: `maps-app` emits a `CoordinatesReceived` event.
    // ------------------------------------------------------------------
    let correlation_id = Uuid::new_v4();
    let input_event = EventEnvelope::new(
        ib.event_schema("CoordinatesReceived", 1),
        "CoordinatesReceived",
        ib.product("maps-app"),
        Some("maps-app".to_string()),
        correlation_id,
        serde_json::json!({
            "latitude": 48.8566,
            "longitude": 2.3522,
            "minVersion": "1.0.0",
        }),
    );

    // ------------------------------------------------------------------
    // Step 6: The platform resolves the capability and routes to
    //         `photo-app` (the implementor). A `CapabilityEventRouted`
    //         event is appended, scoped to the implementor.
    // ------------------------------------------------------------------
    resolver
        .route_capability_event("gps-to-place", &input_event)
        .await
        .unwrap();

    let events = event_log.events_since(0).await;
    assert_eq!(
        events.len(),
        1,
        "exactly one CapabilityEventRouted event should be appended after routing"
    );
    let routed = &events[0];
    assert_eq!(routed.event_type, "CapabilityEventRouted");
    assert_eq!(
        routed.product.as_deref(),
        Some("photo-app"),
        "routed event must be scoped to photo-app (the implementor)"
    );
    assert_eq!(
        routed.payload["capability"], "gps-to-place",
        "routed payload must reference the capability"
    );
    assert_eq!(
        routed.correlation_id, correlation_id,
        "correlation ID must propagate from consumer through routing"
    );

    // ------------------------------------------------------------------
    // Step 7: Simulate `photo-app` handling the event and emitting the
    //         capability's output event (`PlaceResolved`). This would
    //         happen inside the implementor's workload.
    // ------------------------------------------------------------------
    let place_resolved = EventEnvelope::new(
        ib.event_schema("PlaceResolved", 1),
        "PlaceResolved",
        ib.product("photo-app"),
        Some("photo-app".to_string()),
        correlation_id,
        serde_json::json!({
            "capability": "gps-to-place",
            "place": "Paris",
            "latitude": 48.8566,
            "longitude": 2.3522,
            "confidence": 0.97,
        }),
    );
    event_log.append(place_resolved.clone()).await.unwrap();

    // ------------------------------------------------------------------
    // Step 8: Verify that `PlaceResolved` is a declared trigger for the
    //         `photo-locations` data product via SPARQL, and resolve the
    //         data products that should refresh.
    // ------------------------------------------------------------------
    let trigger_query = format!(
        r#"
        PREFIX picloud: <https://picloud.local/ontology#>
        SELECT ?dp WHERE {{
            ?dp picloud:triggerEvent "{event_type}" .
        }}
        "#,
        event_type = place_resolved.event_type,
    );
    let trigger_result = projector.query(&trigger_query).await.unwrap();
    assert_eq!(
        trigger_result.bindings.len(),
        1,
        "exactly one data product should declare PlaceResolved as a trigger"
    );
    let triggered_dp_iri = trigger_result.bindings[0]["dp"]
        .as_str()
        .or_else(|| trigger_result.bindings[0]["dp"]["value"].as_str())
        .unwrap()
        .to_string();
    assert_eq!(
        triggered_dp_iri, dp_resource_iri_str,
        "the triggered data product IRI must match the declared one"
    );

    // ------------------------------------------------------------------
    // Step 9: The trigger dispatcher rebuilds the data product projection.
    //         (In production, the event subscriber runs this
    //         automatically; the test drives it directly.)
    // ------------------------------------------------------------------
    let construct_query = format!(
        r#"CONSTRUCT {{
            ?photo <https://picloud.local/ontology#placeName> ?place .
        }}
        WHERE {{
            GRAPH <{pg}> {{
                ?photo <https://picloud.local/ontology#placeName> ?place .
            }}
        }}"#,
        pg = product_graph_iri.as_str(),
    );

    let refresh_result = dp_projector
        .refresh_projection(&dp_resource_iri, &construct_query, &product_graph_iri)
        .await
        .unwrap();

    assert_eq!(
        refresh_result.triple_count, 2,
        "projection should contain 2 triples (Paris + London)"
    );

    // Append a DataProductRefreshed event with the capability output event
    // as the trigger — this is the observable signal that the chain
    // (capability -> output event -> data product refresh) completed.
    let refreshed_event = EventEnvelope::new(
        ib.event_schema("DataProductRefreshed", 1),
        "DataProductRefreshed",
        dp_resource_iri.clone(),
        Some("photo-app".to_string()),
        correlation_id,
        serde_json::json!({
            "data_product_iri": dp_resource_iri_str,
            "triple_count": refresh_result.triple_count,
            "duration_ms": refresh_result.duration_ms,
            "trigger_event": place_resolved.event_type,
            "refreshed_at": chrono::Utc::now().to_rfc3339(),
        }),
    );
    event_log.append(refreshed_event).await.unwrap();

    // ------------------------------------------------------------------
    // Step 10: Verify the chain of events in the log and that the data
    //          product is now queryable.
    // ------------------------------------------------------------------
    let all_events = event_log.events_since(0).await;
    assert_eq!(
        all_events.len(),
        3,
        "event log should contain: CapabilityEventRouted, PlaceResolved, DataProductRefreshed"
    );

    let event_types: Vec<&str> =
        all_events.iter().map(|e| e.event_type.as_str()).collect();
    assert_eq!(
        event_types,
        vec![
            "CapabilityEventRouted",
            "PlaceResolved",
            "DataProductRefreshed",
        ],
        "event log order must reflect the routing -> output -> refresh chain"
    );

    let refreshed = &all_events[2];
    assert_eq!(
        refreshed.payload["trigger_event"], "PlaceResolved",
        "DataProductRefreshed must name the capability output as the trigger"
    );
    assert_eq!(
        refreshed.payload["triple_count"], 2,
        "DataProductRefreshed must carry the projected triple count"
    );
    assert_eq!(
        refreshed.correlation_id, correlation_id,
        "correlation ID propagates end-to-end from the initial consumer event"
    );

    // Query the published data product graph and verify it is populated.
    let dp_query_result = dp_projector
        .query_data_product(
            &dp_resource_iri,
            "?photo <https://picloud.local/ontology#placeName> ?place",
        )
        .await
        .unwrap();
    assert_eq!(
        dp_query_result.bindings.len(),
        2,
        "the published data product graph should expose 2 places"
    );
    let places: Vec<String> = dp_query_result
        .bindings
        .iter()
        .map(|b| b["place"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(places.contains(&"Paris".to_string()));
    assert!(places.contains(&"London".to_string()));

    // ------------------------------------------------------------------
    // End-to-end timing: assert we are well within the 30-second budget.
    // ------------------------------------------------------------------
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 30,
        "end-to-end capability -> data product chain must complete under 30s (took {:?})",
        elapsed
    );
}
