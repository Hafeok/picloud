/// FT-070 Integration Tests — Data Product Lifecycle Events
///
/// Covers:
///   TC-274: Data product lifecycle events emitted on create, update, delete (scenario)
///   TC-331: Data product events exit — create, update, delete events emitted (exit-criteria)
///
/// Verifies that the three core lifecycle events (DataProductDeclared,
/// DataProductUpdated, DataProductDeleted) are properly emitted, projected
/// into the RDF graph, and that each event correctly mutates the catalog state.

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
    source: ResourceIri,
    product: Option<&str>,
    payload: serde_json::Value,
) -> EventEnvelope {
    let ib = iri_builder();
    EventEnvelope::new(
        ib.event_schema(event_type, 1),
        event_type,
        source,
        product.map(|s| s.to_string()),
        Uuid::new_v4(),
        payload,
    )
}

fn make_product_deployed(name: &str, version: &str) -> EventEnvelope {
    let ib = iri_builder();
    let product_iri = ib.product(name);
    make_event(
        "ProductDeployed",
        product_iri.clone(),
        Some(name),
        serde_json::json!({
            "product_iri": product_iri.as_str(),
            "product_name": name,
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
        ResourceIri::new(&dp_iri_str).unwrap(),
        Some(product),
        payload,
    )
}

fn make_data_product_updated(
    product: &str,
    dp_name: &str,
    domain: &str,
    version: &str,
    max_age: Option<&str>,
    description: Option<&str>,
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
    if let Some(desc) = description {
        payload["description"] = serde_json::Value::String(desc.to_string());
    }
    make_event(
        "DataProductUpdated",
        ResourceIri::new(&dp_iri_str).unwrap(),
        Some(product),
        payload,
    )
}

fn make_data_product_deleted(product: &str, dp_name: &str) -> EventEnvelope {
    let dp_iri_str = dp_resource_iri(product, dp_name);
    make_event(
        "DataProductDeleted",
        ResourceIri::new(&dp_iri_str).unwrap(),
        Some(product),
        serde_json::json!({
            "data_product_iri": dp_iri_str,
            "name": dp_name,
            "product": product,
        }),
    )
}

// ============================================================================
// TC-274 — Data product lifecycle events emitted on create, update, delete
// ============================================================================
/// Scenario test for FT-070:
///
/// 1. Deploy a product ("analytics-app") and declare a data product within it.
///    Verify DataProductDeclared is projected into the RDF catalog with correct
///    metadata (name, product, domain, version, maxAge, status="declared").
///
/// 2. Emit a DataProductUpdated event that bumps the version, changes the
///    domain, and updates the freshness SLO. Verify the RDF catalog reflects
///    all three changes atomically — old values are removed, new values present.
///
/// 3. Emit a DataProductDeleted event. Verify all triples about the data
///    product are removed from every graph.
///
/// 4. Verify the full lifecycle: the same IRI goes from declared → updated → deleted
///    with correct state at each stage.
#[tokio::test]
async fn tc274_data_product_lifecycle_events_emitted_on_create_update_delete() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // ---- Step 0: Deploy the owning product ----
    let deploy = make_product_deployed("analytics-app", "1.0.0");
    projector.project(&deploy).await.unwrap();

    // ---- Step 1: CREATE — DataProductDeclared ----
    let dp_name = "user-engagement";
    let dp_iri = dp_resource_iri("analytics-app", dp_name);

    let declared = make_data_product_declared(
        "analytics-app",
        dp_name,
        "behavioral",
        "1.0.0",
        Some("PT15M"),
    );
    projector.project(&declared).await.unwrap();

    // Verify the data product exists as a pc:DataProduct
    let ask_type = format!(
        "ASK {{ <{dp_iri}> <{RDF_TYPE}> <{PICLOUD_NS}DataProduct> }}"
    );
    let result = projector.query(&ask_type).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product should exist as a pc:DataProduct after DataProductDeclared"
    );

    // Verify name
    let ask_name = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}name> \"{dp_name}\" }}"
    );
    let result = projector.query(&ask_name).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product name should match"
    );

    // Verify product scope
    let ask_product = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}product> \"analytics-app\" }}"
    );
    let result = projector.query(&ask_product).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product should be scoped to analytics-app"
    );

    // Verify initial domain
    let ask_domain = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}domain> \"behavioral\" }}"
    );
    let result = projector.query(&ask_domain).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product should belong to behavioral domain"
    );

    // Verify initial version
    let ask_version = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}version> \"1.0.0\" }}"
    );
    let result = projector.query(&ask_version).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product version should be 1.0.0"
    );

    // Verify initial status is "declared"
    let ask_status = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}status> \"declared\" }}"
    );
    let result = projector.query(&ask_status).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product status should be 'declared' after creation"
    );

    // Verify freshness SLO
    let ask_max_age = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}maxAge> \"PT15M\" }}"
    );
    let result = projector.query(&ask_max_age).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product maxAge should be PT15M"
    );

    // Verify producedBy link
    let product_iri = ib.product("analytics-app");
    let ask_produced_by = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}producedBy> <{}> }}",
        product_iri.as_str()
    );
    let result = projector.query(&ask_produced_by).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product should be linked to analytics-app via producedBy"
    );

    // Verify belongsToDomain link
    let cluster_root = ib.cluster_root();
    let domain_iri = format!(
        "{}/data-domains/behavioral",
        cluster_root.as_str().trim_end_matches('/')
    );
    let ask_belongs = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}belongsToDomain> <{domain_iri}> }}"
    );
    let result = projector.query(&ask_belongs).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product should be linked to behavioral domain via belongsToDomain"
    );

    // ---- Step 2: UPDATE — DataProductUpdated ----
    // Bump version, change domain, update freshness SLO
    let updated = make_data_product_updated(
        "analytics-app",
        dp_name,
        "engagement",   // domain changed: behavioral → engagement
        "2.0.0",        // version bumped: 1.0.0 → 2.0.0
        Some("PT30M"),  // SLO relaxed: 15m → 30m
        Some("Bump version and reassign domain"),
    );
    projector.project(&updated).await.unwrap();

    // Verify version updated
    let ask_version_new = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}version> \"2.0.0\" }}"
    );
    let result = projector.query(&ask_version_new).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "version should be 2.0.0 after update"
    );

    // Verify old version is gone
    let ask_version_old = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}version> \"1.0.0\" }}"
    );
    let result = projector.query(&ask_version_old).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "old version 1.0.0 should be removed after update"
    );

    // Verify domain updated
    let ask_domain_new = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}domain> \"engagement\" }}"
    );
    let result = projector.query(&ask_domain_new).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "domain should be engagement after update"
    );

    // Verify old domain is gone
    let ask_domain_old = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}domain> \"behavioral\" }}"
    );
    let result = projector.query(&ask_domain_old).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "old domain behavioral should be removed after update"
    );

    // Verify belongsToDomain link updated
    let new_domain_iri = format!(
        "{}/data-domains/engagement",
        cluster_root.as_str().trim_end_matches('/')
    );
    let ask_belongs_new = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}belongsToDomain> <{new_domain_iri}> }}"
    );
    let result = projector.query(&ask_belongs_new).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "belongsToDomain should point to engagement domain after update"
    );

    // Verify old domain link is gone
    let ask_belongs_old = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}belongsToDomain> <{domain_iri}> }}"
    );
    let result = projector.query(&ask_belongs_old).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "old belongsToDomain link should be removed after update"
    );

    // Verify maxAge updated
    let ask_max_age_new = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}maxAge> \"PT30M\" }}"
    );
    let result = projector.query(&ask_max_age_new).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "maxAge should be PT30M after update"
    );

    // Verify old maxAge is gone
    let ask_max_age_old = format!(
        "ASK {{ <{dp_iri}> <{PICLOUD_NS}maxAge> \"PT15M\" }}"
    );
    let result = projector.query(&ask_max_age_old).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "old maxAge PT15M should be removed after update"
    );

    // Verify the data product still exists (type, name, product unchanged)
    let result = projector.query(&ask_type).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product should still exist as pc:DataProduct after update"
    );
    let result = projector.query(&ask_name).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product name should be unchanged after update"
    );
    let result = projector.query(&ask_product).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product should still be scoped to analytics-app after update"
    );

    // ---- Step 3: DELETE — DataProductDeleted ----
    let deleted = make_data_product_deleted("analytics-app", dp_name);
    projector.project(&deleted).await.unwrap();

    // Verify the data product no longer exists (no triples about the IRI)
    let result = projector.query(&ask_type).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "data product should not exist after DataProductDeleted"
    );

    // Verify all metadata is gone
    let select_all = format!(
        "SELECT ?p ?o WHERE {{ <{dp_iri}> ?p ?o }}"
    );
    let result = projector.query(&select_all).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        0,
        "no triples should remain about the data product after deletion"
    );
}

// ============================================================================
// TC-331 — Data product events exit — create, update, delete events emitted
// ============================================================================
/// Exit-criteria test for FT-070:
///
/// Verifies the complete lifecycle invariant: a data product can be created,
/// updated multiple times, and then deleted, with each event correctly
/// mutating the RDF catalog. This is the gate criterion — if this passes,
/// the feature is considered complete.
///
/// Exercises:
///   - Two distinct data products in the same owning product
///   - Multiple sequential updates to the same data product
///   - Deletion of one data product while the other survives
///   - Final deletion of the second data product
#[tokio::test]
async fn tc331_data_product_events_exit_create_update_delete_events_emitted() {
    let projector = OxigraphProjector::new().unwrap();

    // Deploy the owning product
    let deploy = make_product_deployed("crm-app", "1.0.0");
    projector.project(&deploy).await.unwrap();

    // ---- Create two data products ----
    let dp_a = "customer-segments";
    let dp_b = "lead-scores";
    let dp_a_iri = dp_resource_iri("crm-app", dp_a);
    let dp_b_iri = dp_resource_iri("crm-app", dp_b);

    let declared_a = make_data_product_declared(
        "crm-app",
        dp_a,
        "marketing",
        "1.0.0",
        Some("PT10M"),
    );
    let declared_b = make_data_product_declared(
        "crm-app",
        dp_b,
        "sales",
        "1.0.0",
        None,
    );
    projector.project(&declared_a).await.unwrap();
    projector.project(&declared_b).await.unwrap();

    // Both should exist
    let ask_a = format!(
        "ASK {{ <{dp_a_iri}> <{RDF_TYPE}> <{PICLOUD_NS}DataProduct> }}"
    );
    let ask_b = format!(
        "ASK {{ <{dp_b_iri}> <{RDF_TYPE}> <{PICLOUD_NS}DataProduct> }}"
    );
    let result = projector.query(&ask_a).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "dp_a should exist after declared");
    let result = projector.query(&ask_b).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "dp_b should exist after declared");

    // ---- Update dp_a twice (sequential version bumps) ----
    let update_a_1 = make_data_product_updated(
        "crm-app",
        dp_a,
        "marketing",
        "1.1.0",
        Some("PT5M"),
        Some("Tighten SLO"),
    );
    projector.project(&update_a_1).await.unwrap();

    // Verify first update
    let ask_version = format!(
        "ASK {{ <{dp_a_iri}> <{PICLOUD_NS}version> \"1.1.0\" }}"
    );
    let result = projector.query(&ask_version).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "dp_a version should be 1.1.0 after first update");

    let ask_max_age = format!(
        "ASK {{ <{dp_a_iri}> <{PICLOUD_NS}maxAge> \"PT5M\" }}"
    );
    let result = projector.query(&ask_max_age).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "dp_a maxAge should be PT5M after first update");

    // Second update — change domain and version again
    let update_a_2 = make_data_product_updated(
        "crm-app",
        dp_a,
        "analytics",   // domain changed
        "2.0.0",       // version bumped again
        Some("PT1H"),  // SLO relaxed
        Some("Major version bump, domain reassignment"),
    );
    projector.project(&update_a_2).await.unwrap();

    // Verify second update
    let ask_version_2 = format!(
        "ASK {{ <{dp_a_iri}> <{PICLOUD_NS}version> \"2.0.0\" }}"
    );
    let result = projector.query(&ask_version_2).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "dp_a version should be 2.0.0 after second update");

    let ask_domain_new = format!(
        "ASK {{ <{dp_a_iri}> <{PICLOUD_NS}domain> \"analytics\" }}"
    );
    let result = projector.query(&ask_domain_new).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "dp_a domain should be analytics after second update");

    // Verify no stale versions remain (only one version triple)
    let select_versions = format!(
        "SELECT ?v WHERE {{ <{dp_a_iri}> <{PICLOUD_NS}version> ?v }}"
    );
    let result = projector.query(&select_versions).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        1,
        "exactly one version triple should exist (no stale duplicates)"
    );

    // ---- Update dp_b ----
    let update_b = make_data_product_updated(
        "crm-app",
        dp_b,
        "sales",
        "1.1.0",
        Some("PT20M"),
        None,
    );
    projector.project(&update_b).await.unwrap();

    // ---- Delete dp_a while dp_b survives ----
    let delete_a = make_data_product_deleted("crm-app", dp_a);
    projector.project(&delete_a).await.unwrap();

    // dp_a should be gone
    let result = projector.query(&ask_a).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "dp_a should not exist after deletion"
    );

    // dp_b should survive
    let result = projector.query(&ask_b).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "dp_b should still exist after dp_a is deleted"
    );

    // Verify dp_b metadata is intact
    let ask_b_version = format!(
        "ASK {{ <{dp_b_iri}> <{PICLOUD_NS}version> \"1.1.0\" }}"
    );
    let result = projector.query(&ask_b_version).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "dp_b version should be 1.1.0 (unaffected by dp_a deletion)"
    );

    let ask_b_max_age = format!(
        "ASK {{ <{dp_b_iri}> <{PICLOUD_NS}maxAge> \"PT20M\" }}"
    );
    let result = projector.query(&ask_b_max_age).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "dp_b maxAge should be PT20M (unaffected by dp_a deletion)"
    );

    // ---- Delete dp_b ----
    let delete_b = make_data_product_deleted("crm-app", dp_b);
    projector.project(&delete_b).await.unwrap();

    // dp_b should be gone
    let result = projector.query(&ask_b).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "dp_b should not exist after deletion"
    );

    // Verify no data product triples remain for either IRI
    let select_a = format!(
        "SELECT ?p ?o WHERE {{ <{dp_a_iri}> ?p ?o }}"
    );
    let result = projector.query(&select_a).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        0,
        "no triples should remain for dp_a"
    );

    let select_b = format!(
        "SELECT ?p ?o WHERE {{ <{dp_b_iri}> ?p ?o }}"
    );
    let result = projector.query(&select_b).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        0,
        "no triples should remain for dp_b"
    );
}
