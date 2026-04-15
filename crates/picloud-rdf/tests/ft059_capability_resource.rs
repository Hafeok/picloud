/// FT-059 Integration Tests — Capability Resource Type
///
/// Covers:
///   TC-232: Capability declared and fulfilled by implementing product (exit-criteria)
///
/// Verifies the full capability lifecycle through RDF projection:
///   1. A capability is declared (CapabilityDeclared) with ontology, SHACL shapes,
///      and input/output event types
///   2. The capability appears in the RDF graph as a pc:Capability with correct
///      metadata triples (version, inputEvent, outputEvent, status="declared")
///   3. A product is deployed that implements the capability
///   4. CapabilityImplementorAdded links the product to the capability
///   5. CapabilityReady transitions the capability to "ready" status
///   6. SPARQL queries confirm the fulfilled state and implementedBy relationship

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
// TC-232 — Capability declared and fulfilled by implementing product
// ============================================================================
/// Exit criteria for FT-059: Declare a capability with ontology, SHACL shapes,
/// and event types. Deploy a product that implements it. Verify the full
/// lifecycle: capability appears in the RDF graph as declared, then transitions
/// to ready when an implementor is added and the CapabilityReady event fires.
/// SPARQL confirms the pc:implementedBy triple links capability to product.
#[tokio::test]
async fn tc232_capability_declared_and_fulfilled() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // --- Step 1: Declare a capability ---
    let cap_declared = make_capability_declared(
        "gps-to-place",
        "1.0.0",
        "CoordinatesReceived",
        "PlaceResolved",
    );
    projector.project(&cap_declared).await.unwrap();

    let capability_iri = ib.cluster_resource("capabilities", "gps-to-place");

    // Verify capability exists as pc:Capability with correct metadata
    let ask_type = format!(
        "ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}Capability> }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_type).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "gps-to-place should exist as a pc:Capability"
    );

    // Verify name
    let ask_name = format!(
        "ASK {{ <{}> <{PICLOUD_NS}name> \"gps-to-place\" }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_name).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "capability should have correct name"
    );

    // Verify version
    let ask_version = format!(
        "ASK {{ <{}> <{PICLOUD_NS}version> \"1.0.0\" }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_version).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "capability should have correct version"
    );

    // Verify inputEvent
    let ask_input = format!(
        "ASK {{ <{}> <{PICLOUD_NS}inputEvent> \"CoordinatesReceived\" }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_input).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "capability should have correct inputEvent"
    );

    // Verify outputEvent
    let ask_output = format!(
        "ASK {{ <{}> <{PICLOUD_NS}outputEvent> \"PlaceResolved\" }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_output).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "capability should have correct outputEvent"
    );

    // Verify initial status is "declared"
    let ask_declared = format!(
        "ASK {{ <{}> <{PICLOUD_NS}status> \"declared\" }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_declared).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "capability status should be 'declared' initially"
    );

    // --- Step 2: Deploy a product that implements the capability ---
    let product_deployed = make_product_deployed("geo-service", "2.0.0");
    projector.project(&product_deployed).await.unwrap();

    // --- Step 3: Add the product as an implementor ---
    let impl_added =
        make_capability_implementor_added("gps-to-place", "geo-service", "2.0.0");
    projector.project(&impl_added).await.unwrap();

    // Verify the implementedBy triple exists
    let product_iri = ib.product("geo-service");
    let ask_impl = format!(
        "ASK {{ <{}> <{PICLOUD_NS}implementedBy> <{}> }}",
        capability_iri.as_str(),
        product_iri.as_str()
    );
    let result = projector.query(&ask_impl).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "capability should be implementedBy the geo-service product"
    );

    // --- Step 4: Mark the capability as ready (fulfilled) ---
    let cap_ready = make_capability_ready("gps-to-place", "geo-service");
    projector.project(&cap_ready).await.unwrap();

    // Verify status transitioned to "ready".
    // update_status stores status as a named node (pc:Ready) and a literal
    // on pc:statusLabel for backward compatibility.
    let ask_ready = format!(
        "ASK {{ <{}> <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_ready).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "capability status should be pc:Ready after implementor is added"
    );

    // Also verify the statusLabel literal is set
    let ask_label = format!(
        "ASK {{ <{}> <{PICLOUD_NS}statusLabel> \"ready\" }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_label).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "capability statusLabel should be 'ready'"
    );

    // Verify the old "declared" literal status is gone (update_status replaces it)
    let ask_still_declared = format!(
        "ASK {{ <{}> <{PICLOUD_NS}status> \"declared\" }}",
        capability_iri.as_str()
    );
    let result = projector.query(&ask_still_declared).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "capability should no longer have 'declared' status after becoming ready"
    );

    // --- Step 5: Cross-cutting query — find all fulfilled capabilities ---
    // Use the named node form for status matching
    let fulfilled_query = format!(
        "SELECT ?cap ?name ?impl WHERE {{ \
         ?cap <{RDF_TYPE}> <{PICLOUD_NS}Capability> ; \
              <{PICLOUD_NS}name> ?name ; \
              <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> ; \
              <{PICLOUD_NS}implementedBy> ?impl . \
         }}"
    );
    let result = projector.query(&fulfilled_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        1,
        "exactly one fulfilled capability should be returned"
    );
    // SELECT bindings return JSON objects like {"type": "literal", "value": "gps-to-place"}
    assert_eq!(
        result.bindings[0]["name"]["value"], "gps-to-place",
        "the fulfilled capability should be gps-to-place"
    );
}
