/// FT-071 Integration Tests — Data Domain Lifecycle Events
///
/// Covers:
///   TC-275: Data domain lifecycle events emitted on create, update, delete (scenario)
///   TC-332: Data domain events exit — create, update, delete events emitted (exit-criteria)
///
/// Verifies that the three core lifecycle events (DataDomainDeclared,
/// DataDomainUpdated, DataDomainDeleted) are properly emitted, projected
/// into the RDF graph, and that each event correctly mutates the governance state.

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

fn make_data_domain_declared(
    name: &str,
    steward: &str,
    sensitivity: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let domain_iri = ib.cluster_resource("data-domains", name);
    make_event(
        "DataDomainDeclared",
        None, // cluster-scoped, no product
        serde_json::json!({
            "domain_iri": domain_iri.as_str(),
            "name": name,
            "steward": steward,
            "sensitivity": sensitivity,
        }),
    )
}

fn make_data_domain_updated(
    name: &str,
    steward: &str,
    sensitivity: &str,
    description: Option<&str>,
) -> EventEnvelope {
    let ib = iri_builder();
    let domain_iri = ib.cluster_resource("data-domains", name);
    let mut payload = serde_json::json!({
        "domain_iri": domain_iri.as_str(),
        "name": name,
        "steward": steward,
        "sensitivity": sensitivity,
    });
    if let Some(desc) = description {
        payload["description"] = serde_json::Value::String(desc.to_string());
    }
    make_event(
        "DataDomainUpdated",
        None, // cluster-scoped, no product
        payload,
    )
}

fn make_data_domain_deleted(name: &str) -> EventEnvelope {
    let ib = iri_builder();
    let domain_iri = ib.cluster_resource("data-domains", name);
    make_event(
        "DataDomainDeleted",
        None,
        serde_json::json!({
            "domain_iri": domain_iri.as_str(),
            "name": name,
        }),
    )
}

// ============================================================================
// TC-275 — Data domain lifecycle events emitted on create, update, delete
// ============================================================================
/// Scenario test for FT-071:
///
/// 1. Declare a data domain ("customer-analytics") with steward and sensitivity.
///    Verify DataDomainDeclared is projected into the RDF graph with correct
///    metadata (name, steward, sensitivity, status="declared").
///
/// 2. Emit a DataDomainUpdated event that reassigns the steward and
///    reclassifies the sensitivity. Verify the RDF graph reflects both
///    changes atomically — old values are removed, new values present.
///
/// 3. Emit a DataDomainDeleted event. Verify all triples about the data
///    domain are removed from every graph.
///
/// 4. Verify the full lifecycle: the same IRI goes from declared -> updated -> deleted
///    with correct state at each stage.
#[tokio::test]
async fn tc275_data_domain_lifecycle_events_emitted_on_create_update_delete() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // ---- Step 1: CREATE — DataDomainDeclared ----
    let domain_name = "customer-analytics";
    let domain_iri = ib.cluster_resource("data-domains", domain_name);

    let declared = make_data_domain_declared(
        domain_name,
        "https://picloud.local/identities/data-steward-alice",
        "confidential",
    );
    projector.project(&declared).await.unwrap();

    // Verify the data domain exists as a pc:DataDomain
    let ask_type = format!(
        "ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_type).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data domain should exist as a pc:DataDomain after DataDomainDeclared"
    );

    // Verify name
    let ask_name = format!(
        "ASK {{ <{}> <{PICLOUD_NS}name> \"{domain_name}\" }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_name).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data domain name should match"
    );

    // Verify steward
    let ask_steward = format!(
        "ASK {{ <{}> <{PICLOUD_NS}steward> \"https://picloud.local/identities/data-steward-alice\" }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_steward).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data domain steward should be alice"
    );

    // Verify sensitivity
    let ask_sensitivity = format!(
        "ASK {{ <{}> <{PICLOUD_NS}sensitivity> \"confidential\" }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_sensitivity).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data domain sensitivity should be confidential"
    );

    // Verify initial status is "declared"
    let ask_status = format!(
        "ASK {{ <{}> <{PICLOUD_NS}status> \"declared\" }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_status).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data domain status should be 'declared' after creation"
    );

    // ---- Step 2: UPDATE — DataDomainUpdated ----
    // Reassign steward and reclassify sensitivity
    let updated = make_data_domain_updated(
        domain_name,
        "https://picloud.local/identities/data-steward-bob", // steward changed: alice -> bob
        "restricted", // sensitivity escalated: confidential -> restricted
        Some("Reassign steward and escalate sensitivity"),
    );
    projector.project(&updated).await.unwrap();

    // Verify steward updated
    let ask_steward_new = format!(
        "ASK {{ <{}> <{PICLOUD_NS}steward> \"https://picloud.local/identities/data-steward-bob\" }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_steward_new).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "steward should be bob after update"
    );

    // Verify old steward is gone
    let ask_steward_old = format!(
        "ASK {{ <{}> <{PICLOUD_NS}steward> \"https://picloud.local/identities/data-steward-alice\" }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_steward_old).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "old steward alice should be removed after update"
    );

    // Verify sensitivity updated
    let ask_sensitivity_new = format!(
        "ASK {{ <{}> <{PICLOUD_NS}sensitivity> \"restricted\" }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_sensitivity_new).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "sensitivity should be restricted after update"
    );

    // Verify old sensitivity is gone
    let ask_sensitivity_old = format!(
        "ASK {{ <{}> <{PICLOUD_NS}sensitivity> \"confidential\" }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_sensitivity_old).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "old sensitivity confidential should be removed after update"
    );

    // Verify the data domain still exists (type, name, status unchanged)
    let result = projector.query(&ask_type).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data domain should still exist as pc:DataDomain after update"
    );
    let result = projector.query(&ask_name).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data domain name should be unchanged after update"
    );
    let result = projector.query(&ask_status).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data domain status should still be 'declared' after update"
    );

    // ---- Step 3: DELETE — DataDomainDeleted ----
    let deleted = make_data_domain_deleted(domain_name);
    projector.project(&deleted).await.unwrap();

    // Verify the data domain no longer exists (no triples about the IRI)
    let result = projector.query(&ask_type).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "data domain should not exist after DataDomainDeleted"
    );

    // Verify all metadata is gone
    let select_all = format!(
        "SELECT ?p ?o WHERE {{ <{}> ?p ?o }}",
        domain_iri.as_str()
    );
    let result = projector.query(&select_all).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        0,
        "no triples should remain about the data domain after deletion"
    );
}

// ============================================================================
// TC-332 — Data domain events exit — create, update, delete events emitted
// ============================================================================
/// Exit-criteria test for FT-071:
///
/// Verifies the complete lifecycle invariant: a data domain can be created,
/// updated multiple times, and then deleted, with each event correctly
/// mutating the RDF graph. This is the gate criterion — if this passes,
/// the feature is considered complete.
///
/// Exercises:
///   - Two distinct data domains with different sensitivity levels
///   - Multiple sequential updates to the same data domain
///   - Deletion of one data domain while the other survives
///   - Final deletion of the second data domain
#[tokio::test]
async fn tc332_data_domain_events_exit_create_update_delete_events_emitted() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // ---- Create two data domains ----
    let domain_a = "financial-data";
    let domain_b = "operational-metrics";
    let domain_a_iri = ib.cluster_resource("data-domains", domain_a);
    let domain_b_iri = ib.cluster_resource("data-domains", domain_b);

    let declared_a = make_data_domain_declared(
        domain_a,
        "https://picloud.local/identities/steward-finance",
        "restricted",
    );
    let declared_b = make_data_domain_declared(
        domain_b,
        "https://picloud.local/identities/steward-ops",
        "internal",
    );
    projector.project(&declared_a).await.unwrap();
    projector.project(&declared_b).await.unwrap();

    // Both should exist
    let ask_a = format!(
        "ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> }}",
        domain_a_iri.as_str()
    );
    let ask_b = format!(
        "ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> }}",
        domain_b_iri.as_str()
    );
    let result = projector.query(&ask_a).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "domain_a should exist after declared");
    let result = projector.query(&ask_b).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "domain_b should exist after declared");

    // Verify both are discoverable in a single query
    let select_all = format!(
        "SELECT ?domain ?name WHERE {{ \
         ?domain <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> ; \
                 <{PICLOUD_NS}name> ?name . \
         }} ORDER BY ?name"
    );
    let result = projector.query(&select_all).await.unwrap();
    assert_eq!(result.bindings.len(), 2, "both data domains should be discoverable");

    // ---- Update domain_a twice (sequential governance changes) ----

    // First update: reassign steward
    let update_a_1 = make_data_domain_updated(
        domain_a,
        "https://picloud.local/identities/steward-compliance",
        "restricted", // sensitivity unchanged
        Some("Reassign steward to compliance team"),
    );
    projector.project(&update_a_1).await.unwrap();

    // Verify steward updated
    let ask_steward_a = format!(
        "ASK {{ <{}> <{PICLOUD_NS}steward> \"https://picloud.local/identities/steward-compliance\" }}",
        domain_a_iri.as_str()
    );
    let result = projector.query(&ask_steward_a).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "domain_a steward should be steward-compliance after first update"
    );

    // Verify old steward is gone
    let ask_old_steward_a = format!(
        "ASK {{ <{}> <{PICLOUD_NS}steward> \"https://picloud.local/identities/steward-finance\" }}",
        domain_a_iri.as_str()
    );
    let result = projector.query(&ask_old_steward_a).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "old steward-finance should be gone after first update"
    );

    // Second update: also reclassify sensitivity
    let update_a_2 = make_data_domain_updated(
        domain_a,
        "https://picloud.local/identities/steward-compliance",
        "confidential", // sensitivity downgraded: restricted -> confidential
        Some("Reclassify as confidential after review"),
    );
    projector.project(&update_a_2).await.unwrap();

    // Verify sensitivity updated
    let ask_sensitivity_a = format!(
        "ASK {{ <{}> <{PICLOUD_NS}sensitivity> \"confidential\" }}",
        domain_a_iri.as_str()
    );
    let result = projector.query(&ask_sensitivity_a).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "domain_a sensitivity should be confidential after second update"
    );

    // Verify old sensitivity is gone
    let ask_old_sensitivity_a = format!(
        "ASK {{ <{}> <{PICLOUD_NS}sensitivity> \"restricted\" }}",
        domain_a_iri.as_str()
    );
    let result = projector.query(&ask_old_sensitivity_a).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "old restricted sensitivity should be gone after second update"
    );

    // ---- Update domain_b once ----
    let update_b = make_data_domain_updated(
        domain_b,
        "https://picloud.local/identities/steward-sre",
        "confidential", // sensitivity escalated: internal -> confidential
        None,
    );
    projector.project(&update_b).await.unwrap();

    let ask_sensitivity_b = format!(
        "ASK {{ <{}> <{PICLOUD_NS}sensitivity> \"confidential\" }}",
        domain_b_iri.as_str()
    );
    let result = projector.query(&ask_sensitivity_b).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "domain_b sensitivity should be confidential after update"
    );

    // ---- Delete domain_a while domain_b survives ----
    let deleted_a = make_data_domain_deleted(domain_a);
    projector.project(&deleted_a).await.unwrap();

    // domain_a should be gone
    let result = projector.query(&ask_a).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "domain_a should not exist after deletion"
    );

    // domain_b should still exist
    let result = projector.query(&ask_b).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "domain_b should still exist after deleting domain_a"
    );

    // Only one domain should remain in the catalog
    let result = projector.query(&select_all).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        1,
        "only domain_b should remain after deleting domain_a"
    );
    assert_eq!(
        result.bindings[0]["name"]["value"], "operational-metrics",
        "surviving domain should be operational-metrics"
    );

    // ---- Delete domain_b — catalog should be empty ----
    let deleted_b = make_data_domain_deleted(domain_b);
    projector.project(&deleted_b).await.unwrap();

    let result = projector.query(&ask_b).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "domain_b should not exist after deletion"
    );

    // No domains should remain
    let result = projector.query(&select_all).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        0,
        "no data domains should remain after deleting both"
    );
}
