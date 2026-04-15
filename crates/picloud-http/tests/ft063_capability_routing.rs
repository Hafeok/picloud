/// FT-063 Integration Tests — Capability-aware event routing
///
/// Covers:
///   TC-270: Event routed to implementing product resolved by capability IRI (scenario)
///   TC-327: Capability routing exit — events routed to implementing product (exit-criteria)
///
/// Verifies that the platform resolves the implementing Product at dispatch time
/// when routing events through a declared capability. The CapabilityResolverImpl
/// queries the RDF graph (via SPARQL) to find the implementor whose capability
/// version satisfies the consumer's minVersion, then appends a
/// CapabilityEventRouted event scoped to that implementor's product.

use std::sync::Arc;

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{CapabilityResolver, EventLog, StateProjector};
use picloud_events::InMemoryEventLog;
use picloud_http::CapabilityResolverImpl;
use picloud_rdf::OxigraphProjector;
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
