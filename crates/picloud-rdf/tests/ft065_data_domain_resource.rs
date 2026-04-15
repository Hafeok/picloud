/// FT-065 Integration Tests — Data Domain Resource Type (Cluster-Scoped Governance Boundary)
///
/// Covers:
///   TC-272: Data domain created as cluster-scoped governance boundary (scenario)
///   TC-329: Data domain exit — cluster-scoped governance boundary created (exit-criteria)
///
/// Verifies the data domain lifecycle through RDF projection:
///   1. A data domain is declared (DataDomainDeclared) with steward, sensitivity,
///      and governance metadata
///   2. The data domain appears in the RDF graph as a pc:DataDomain at a cluster-scoped
///      IRI (not product-scoped) with correct metadata triples
///   3. The data domain is cluster-scoped: its IRI is under /data-domains/{name},
///      not under /products/{product}/...
///   4. Multiple data domains can coexist with different sensitivity levels
///   5. A data domain can be deleted (DataDomainDeleted) and all its triples are removed

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
// TC-272 — Data domain created as cluster-scoped governance boundary
// ============================================================================
/// Scenario: Declare a data domain with steward, sensitivity classification,
/// and verify it is projected into the RDF graph as a cluster-scoped
/// pc:DataDomain resource. Verify:
///   - Type is pc:DataDomain
///   - IRI is cluster-scoped (under /data-domains/, not product-scoped)
///   - Has correct name, steward, sensitivity, and status triples
///   - Multiple domains coexist independently
///   - Deletion removes all triples
#[tokio::test]
async fn tc272_data_domain_created_as_cluster_scoped_governance_boundary() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // --- Step 1: Declare a data domain ---
    let domain_declared = make_data_domain_declared(
        "customer-analytics",
        "https://picloud.local/identities/data-steward-alice",
        "confidential",
    );
    projector.project(&domain_declared).await.unwrap();

    let domain_iri = ib.cluster_resource("data-domains", "customer-analytics");

    // Verify the data domain exists as a pc:DataDomain
    let ask_type = format!(
        "ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_type).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "customer-analytics should exist as a pc:DataDomain"
    );

    // --- Step 2: Verify cluster-scoped IRI structure ---
    // The IRI must be at /data-domains/{name}, NOT under /products/{product}/...
    assert!(
        domain_iri.as_str().contains("/data-domains/customer-analytics"),
        "data domain IRI should be cluster-scoped under /data-domains/"
    );
    assert!(
        !domain_iri.as_str().contains("/products/"),
        "data domain IRI must NOT be product-scoped"
    );

    // --- Step 3: Verify all metadata triples ---

    // Verify name
    let ask_name = format!(
        "ASK {{ <{}> <{PICLOUD_NS}name> \"customer-analytics\" }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_name).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data domain should have correct name"
    );

    // Verify steward
    let ask_steward = format!(
        "ASK {{ <{}> <{PICLOUD_NS}steward> \"https://picloud.local/identities/data-steward-alice\" }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_steward).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data domain should have correct steward"
    );

    // Verify sensitivity classification
    let ask_sensitivity = format!(
        "ASK {{ <{}> <{PICLOUD_NS}sensitivity> \"confidential\" }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_sensitivity).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data domain should have correct sensitivity classification"
    );

    // Verify initial status is "declared"
    let ask_status = format!(
        "ASK {{ <{}> <{PICLOUD_NS}status> \"declared\" }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_status).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data domain status should be 'declared' initially"
    );

    // --- Step 4: Declare a second data domain with different sensitivity ---
    let domain2_declared = make_data_domain_declared(
        "public-catalog",
        "https://picloud.local/identities/data-steward-bob",
        "public",
    );
    projector.project(&domain2_declared).await.unwrap();

    let domain2_iri = ib.cluster_resource("data-domains", "public-catalog");

    // Verify second domain exists independently
    let ask_type2 = format!(
        "ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> }}",
        domain2_iri.as_str()
    );
    let result = projector.query(&ask_type2).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "public-catalog should exist as a pc:DataDomain"
    );

    // Verify second domain has its own sensitivity
    let ask_sensitivity2 = format!(
        "ASK {{ <{}> <{PICLOUD_NS}sensitivity> \"public\" }}",
        domain2_iri.as_str()
    );
    let result = projector.query(&ask_sensitivity2).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "public-catalog should have 'public' sensitivity"
    );

    // --- Step 5: Query all data domains — both should appear ---
    let select_all = format!(
        "SELECT ?domain ?name ?sensitivity WHERE {{ \
         ?domain <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> ; \
                 <{PICLOUD_NS}name> ?name ; \
                 <{PICLOUD_NS}sensitivity> ?sensitivity . \
         }} ORDER BY ?name"
    );
    let result = projector.query(&select_all).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        2,
        "exactly two data domains should exist"
    );
    assert_eq!(
        result.bindings[0]["name"]["value"], "customer-analytics",
        "first domain should be customer-analytics (alphabetical)"
    );
    assert_eq!(
        result.bindings[1]["name"]["value"], "public-catalog",
        "second domain should be public-catalog (alphabetical)"
    );

    // --- Step 6: Delete the first domain and verify removal ---
    let domain_deleted = make_data_domain_deleted("customer-analytics");
    projector.project(&domain_deleted).await.unwrap();

    // Verify deleted domain is gone
    let ask_gone = format!(
        "ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_gone).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "customer-analytics should no longer exist after deletion"
    );

    // Verify second domain still exists
    let ask_still = format!(
        "ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> }}",
        domain2_iri.as_str()
    );
    let result = projector.query(&ask_still).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "public-catalog should still exist after deleting customer-analytics"
    );

    // Only one domain should remain
    let select_remaining = format!(
        "SELECT ?domain WHERE {{ \
         ?domain <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> . \
         }}"
    );
    let result = projector.query(&select_remaining).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        1,
        "exactly one data domain should remain after deletion"
    );
}

// ============================================================================
// TC-329 — Data domain exit — cluster-scoped governance boundary created
// ============================================================================
/// Exit criteria: verify the complete governance boundary contract:
///   1. Data domain is created at a cluster-scoped IRI
///   2. It carries the full governance metadata (steward, sensitivity, status)
///   3. It is discoverable via SPARQL as a pc:DataDomain
///   4. All four sensitivity levels (public, internal, confidential, restricted)
///      are valid and correctly stored
///   5. The domain IRI follows the pattern https://picloud.local/data-domains/{name}
#[tokio::test]
async fn tc329_data_domain_exit_cluster_scoped_governance_boundary_created() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // --- Verify all four sensitivity levels work correctly ---
    let sensitivities = [
        ("domain-public", "public"),
        ("domain-internal", "internal"),
        ("domain-confidential", "confidential"),
        ("domain-restricted", "restricted"),
    ];

    for (name, sensitivity) in &sensitivities {
        let declared = make_data_domain_declared(
            name,
            "https://picloud.local/identities/steward",
            sensitivity,
        );
        projector.project(&declared).await.unwrap();
    }

    // Verify all four domains were created
    let count_query = format!(
        "SELECT ?domain ?sensitivity WHERE {{ \
         ?domain <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> ; \
                 <{PICLOUD_NS}sensitivity> ?sensitivity . \
         }} ORDER BY ?sensitivity"
    );
    let result = projector.query(&count_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        4,
        "all four sensitivity-level data domains should be created"
    );

    // Verify each sensitivity level is correct
    let sensitivities_found: Vec<String> = result
        .bindings
        .iter()
        .map(|b| b["sensitivity"]["value"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        sensitivities_found.contains(&"public".to_string()),
        "public sensitivity should exist"
    );
    assert!(
        sensitivities_found.contains(&"internal".to_string()),
        "internal sensitivity should exist"
    );
    assert!(
        sensitivities_found.contains(&"confidential".to_string()),
        "confidential sensitivity should exist"
    );
    assert!(
        sensitivities_found.contains(&"restricted".to_string()),
        "restricted sensitivity should exist"
    );

    // --- Verify cluster-scoped IRI contract for each domain ---
    for (name, _) in &sensitivities {
        let domain_iri = ib.cluster_resource("data-domains", name);

        // IRI must be cluster-scoped
        assert!(
            domain_iri
                .as_str()
                .starts_with("https://picloud.local/data-domains/"),
            "data domain IRI must be cluster-scoped at /data-domains/"
        );

        // IRI must NOT be product-scoped
        assert!(
            !domain_iri.as_str().contains("/products/"),
            "data domain IRI must not be product-scoped"
        );

        // Verify the domain is discoverable in the RDF graph
        let ask_exists = format!(
            "ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> }}",
            domain_iri.as_str()
        );
        let result = projector.query(&ask_exists).await.unwrap();
        assert_eq!(
            result.bindings[0]["result"], true,
            "data domain {} should be discoverable via SPARQL",
            name
        );

        // Verify governance metadata completeness (steward, sensitivity, status)
        let governance_query = format!(
            "ASK {{ \
             <{iri}> <{PICLOUD_NS}steward> ?steward ; \
                     <{PICLOUD_NS}sensitivity> ?sensitivity ; \
                     <{PICLOUD_NS}status> ?status ; \
                     <{PICLOUD_NS}name> ?name . \
             }}",
            iri = domain_iri.as_str()
        );
        let result = projector.query(&governance_query).await.unwrap();
        assert_eq!(
            result.bindings[0]["result"], true,
            "data domain {} must have complete governance metadata (steward, sensitivity, status, name)",
            name
        );
    }

    // --- Cross-cutting query: find all cluster-scoped data domains ---
    // This proves they are discoverable as governance boundaries
    let governance_query = format!(
        "SELECT ?domain ?name ?steward ?sensitivity ?status WHERE {{ \
         ?domain <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> ; \
                 <{PICLOUD_NS}name> ?name ; \
                 <{PICLOUD_NS}steward> ?steward ; \
                 <{PICLOUD_NS}sensitivity> ?sensitivity ; \
                 <{PICLOUD_NS}status> ?status . \
         }} ORDER BY ?name"
    );
    let result = projector.query(&governance_query).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        4,
        "all four governance boundaries should be discoverable via SPARQL"
    );

    // Every domain should have "declared" status
    for binding in &result.bindings {
        assert_eq!(
            binding["status"]["value"], "declared",
            "all newly created data domains should have 'declared' status"
        );
    }
}
