/// FT-062 Integration Tests — Capability Lifecycle Events
///
/// Covers:
///   TC-269: Capability lifecycle events emitted on declare, implement, consume (scenario)
///   TC-326: Capability events exit — declare, implement, consume events emitted (exit-criteria)
///
/// Verifies that the three core capability lifecycle events are correctly emitted
/// and projected into the RDF graph:
///   1. CapabilityDeclared — a capability is created with input/output event types
///   2. CapabilityImplementorAdded — a product declares it implements the capability
///   3. CapabilityConsumerAdded — a product declares it depends on the capability

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

// ============================================================================
// TC-269 — Capability lifecycle events emitted on declare, implement, consume
// ============================================================================
/// Scenario test for FT-062: Exercises the full capability lifecycle through
/// RDF projection — declare a capability, have one product implement it and
/// another product consume it, then verify all three lifecycle events are
/// correctly represented in the RDF graph.
#[tokio::test]
async fn tc269_capability_lifecycle_events_emitted_on_declare_implement_consume() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // ---- Step 1: Declare a capability ----
    let cap_declared = make_capability_declared(
        "image-resize",
        "2.0.0",
        "ImageUploadReceived",
        "ImageResized",
    );
    projector.project(&cap_declared).await.unwrap();

    let capability_iri = ib.cluster_resource("capabilities", "image-resize");

    // Verify the capability exists as a pc:Capability
    let ask_type = format!(
        "ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}Capability> }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_type).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "image-resize should exist as a pc:Capability after CapabilityDeclared"
    );

    // Verify metadata: name, version, inputEvent, outputEvent
    let ask_name = format!(
        "ASK {{ <{}> <{PICLOUD_NS}name> \"image-resize\" }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_name).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "capability should have name");

    let ask_version = format!(
        "ASK {{ <{}> <{PICLOUD_NS}version> \"2.0.0\" }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_version).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "capability should have version");

    let ask_input = format!(
        "ASK {{ <{}> <{PICLOUD_NS}inputEvent> \"ImageUploadReceived\" }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_input).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "capability should have inputEvent");

    let ask_output = format!(
        "ASK {{ <{}> <{PICLOUD_NS}outputEvent> \"ImageResized\" }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_output).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "capability should have outputEvent");

    // Verify initial status is "declared"
    let ask_status = format!(
        "ASK {{ <{}> <{PICLOUD_NS}status> \"declared\" }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_status).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "capability status should be 'declared' initially"
    );

    // ---- Step 2: A product implements the capability ----
    let impl_added = make_capability_implementor_added("image-resize", "media-service", "2.0.0");
    projector.project(&impl_added).await.unwrap();

    let media_product_iri = ib.product("media-service");
    let ask_impl = format!(
        "ASK {{ <{}> <{PICLOUD_NS}implementedBy> <{}> }}",
        capability_iri.as_str(),
        media_product_iri.as_str()
    );
    let result = projector.query(&ask_impl).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "capability should be implementedBy media-service after CapabilityImplementorAdded"
    );

    // Mark capability as ready
    let cap_ready = make_capability_ready("image-resize", "media-service");
    projector.project(&cap_ready).await.unwrap();

    // Verify status transitioned to ready
    let ask_ready = format!(
        "ASK {{ <{}> <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_ready).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "capability status should be pc:Ready after implementor is added"
    );

    // ---- Step 3: Another product consumes the capability ----
    let consumer_added = make_capability_consumer_added("image-resize", "photo-app", "1.0.0");
    projector.project(&consumer_added).await.unwrap();

    let photo_product_iri = ib.product("photo-app");
    let ask_consumer = format!(
        "ASK {{ <{}> <{PICLOUD_NS}consumedBy> <{}> }}",
        capability_iri.as_str(),
        photo_product_iri.as_str()
    );
    let result = projector.query(&ask_consumer).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "capability should be consumedBy photo-app after CapabilityConsumerAdded"
    );

    // ---- Step 4: Cross-cutting query — verify full lifecycle state ----
    // Find the capability with its implementor and consumer
    let lifecycle_query = format!(
        "SELECT ?cap ?name ?impl ?consumer WHERE {{ \
         ?cap <{RDF_TYPE}> <{PICLOUD_NS}Capability> ; \
              <{PICLOUD_NS}name> ?name ; \
              <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> ; \
              <{PICLOUD_NS}implementedBy> ?impl ; \
              <{PICLOUD_NS}consumedBy> ?consumer . \
         }}"
    );
    let result = projector.query(&lifecycle_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        1,
        "exactly one capability should be found with implementor and consumer"
    );
    assert_eq!(
        result.bindings[0]["name"]["value"], "image-resize",
        "the capability should be image-resize"
    );
    assert_eq!(
        result.bindings[0]["impl"]["value"],
        media_product_iri.as_str(),
        "implementor should be media-service"
    );
    assert_eq!(
        result.bindings[0]["consumer"]["value"],
        photo_product_iri.as_str(),
        "consumer should be photo-app"
    );

    // ---- Step 5: Add a second consumer ----
    let consumer2_added = make_capability_consumer_added("image-resize", "gallery-app", "2.0.0");
    projector.project(&consumer2_added).await.unwrap();

    let gallery_product_iri = ib.product("gallery-app");
    let ask_consumer2 = format!(
        "ASK {{ <{}> <{PICLOUD_NS}consumedBy> <{}> }}",
        capability_iri.as_str(),
        gallery_product_iri.as_str()
    );
    let result = projector.query(&ask_consumer2).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "capability should also be consumedBy gallery-app"
    );

    // Verify both consumers exist simultaneously
    let multi_consumer_query = format!(
        "SELECT ?consumer WHERE {{ \
         <{}> <{PICLOUD_NS}consumedBy> ?consumer . \
         }} ORDER BY ?consumer",
        capability_iri.as_str()
    );
    let result = projector.query(&multi_consumer_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        2,
        "capability should have exactly two consumers"
    );
}

// ============================================================================
// TC-326 — Capability events exit — declare, implement, consume events emitted
// ============================================================================
/// Exit criteria for FT-062: Verify that the three core capability lifecycle
/// events (CapabilityDeclared, CapabilityImplementorAdded, CapabilityConsumerAdded)
/// are each emitted and correctly projected into the RDF graph. Each event must
/// produce the expected triples — this is the minimum bar for the feature.
#[tokio::test]
async fn tc326_capability_events_exit_declare_implement_consume_events_emitted() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // ---- Declare ----
    let cap_declared = make_capability_declared(
        "payment-process",
        "1.0.0",
        "PaymentRequested",
        "PaymentProcessed",
    );
    projector.project(&cap_declared).await.unwrap();

    let capability_iri = ib.cluster_resource("capabilities", "payment-process");

    // Verify CapabilityDeclared projected correctly
    let ask_exists = format!(
        "ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}Capability> ; \
                     <{PICLOUD_NS}name> \"payment-process\" ; \
                     <{PICLOUD_NS}version> \"1.0.0\" ; \
                     <{PICLOUD_NS}inputEvent> \"PaymentRequested\" ; \
                     <{PICLOUD_NS}outputEvent> \"PaymentProcessed\" ; \
                     <{PICLOUD_NS}status> \"declared\" . }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_exists).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "CapabilityDeclared must create a fully-described pc:Capability in the graph"
    );

    // ---- Implement ----
    let impl_added =
        make_capability_implementor_added("payment-process", "stripe-adapter", "1.0.0");
    projector.project(&impl_added).await.unwrap();

    let implementor_iri = ib.product("stripe-adapter");
    let ask_impl = format!(
        "ASK {{ <{}> <{PICLOUD_NS}implementedBy> <{}> }}",
        capability_iri.as_str(),
        implementor_iri.as_str()
    );
    let result = projector.query(&ask_impl).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "CapabilityImplementorAdded must create an implementedBy triple"
    );

    // ---- Consume ----
    let consumer_added =
        make_capability_consumer_added("payment-process", "checkout-service", "1.0.0");
    projector.project(&consumer_added).await.unwrap();

    let consumer_iri = ib.product("checkout-service");
    let ask_consumer = format!(
        "ASK {{ <{}> <{PICLOUD_NS}consumedBy> <{}> }}",
        capability_iri.as_str(),
        consumer_iri.as_str()
    );
    let result = projector.query(&ask_consumer).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "CapabilityConsumerAdded must create a consumedBy triple"
    );

    // ---- Verify all three relationships coexist ----
    let full_check = format!(
        "ASK {{ <{cap}> <{RDF_TYPE}> <{PICLOUD_NS}Capability> ; \
                        <{PICLOUD_NS}implementedBy> <{impl_iri}> ; \
                        <{PICLOUD_NS}consumedBy> <{cons_iri}> . }}",
        cap = capability_iri.as_str(),
        impl_iri = implementor_iri.as_str(),
        cons_iri = consumer_iri.as_str(),
    );
    let result = projector.query(&full_check).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "capability must have both implementedBy and consumedBy triples after all lifecycle events"
    );
}
