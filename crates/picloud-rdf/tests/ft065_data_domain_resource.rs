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

fn make_data_domain_declared_with_description(
    name: &str,
    steward: &str,
    sensitivity: &str,
    description: &str,
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
            "description": description,
        }),
    )
}

fn make_data_product_declared(
    name: &str,
    product: &str,
    domain: &str,
    version: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let dp_iri = format!(
        "{}/data-products/{name}",
        ib.product(product).as_str().trim_end_matches('/')
    );
    make_event(
        "DataProductDeclared",
        Some(product),
        serde_json::json!({
            "data_product_iri": dp_iri,
            "name": name,
            "product": product,
            "domain": domain,
            "version": version,
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

// ============================================================================
// TC-196 / TC-208 — data_domain_declaration (scenario + full lifecycle)
// ============================================================================
/// Scenario: declare a `data-domain` resource with steward, sensitivity, and
/// description fields. Verify:
///   TC-196:
///   - `DataDomainDeclared` event projects all governance triples
///   - `pc:steward`, `pc:sensitivity`, `pc:description` triples are present
///   TC-208 (end-to-end lifecycle):
///   - Domain has a dereferenceable cluster-scoped IRI
///   - A second declaration with the same name is rejected as a duplicate
///   - The domain cannot be deleted while a data product is assigned
#[tokio::test]
async fn data_domain_declaration() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // --- TC-196: Declare a data-domain with description ---
    let declared = make_data_domain_declared_with_description(
        "geospatial",
        "https://picloud.local/platform/identities/alice",
        "internal",
        "All location and mapping data products across the cluster",
    );
    projector.project(&declared).await.unwrap();

    let domain_iri = ib.cluster_resource("data-domains", "geospatial");

    // Verify the domain exists as a pc:DataDomain
    let ask_type = format!(
        "ASK {{ <{}> <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_type).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "geospatial should exist as a pc:DataDomain"
    );

    // Verify pc:steward triple
    let ask_steward = format!(
        "ASK {{ <{}> <{PICLOUD_NS}steward> \"https://picloud.local/platform/identities/alice\" }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_steward).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data domain should have pc:steward triple"
    );

    // Verify pc:sensitivity triple
    let ask_sensitivity = format!(
        "ASK {{ <{}> <{PICLOUD_NS}sensitivity> \"internal\" }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_sensitivity).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data domain should have pc:sensitivity triple"
    );

    // Verify pc:description triple (TC-196 requirement)
    let ask_description = format!(
        "ASK {{ <{}> <{PICLOUD_NS}description> \"All location and mapping data products across the cluster\" }}",
        domain_iri.as_str()
    );
    let result = projector.query(&ask_description).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data domain should have pc:description triple"
    );

    // --- TC-208: Dereferenceable cluster-scoped IRI ---
    assert!(
        domain_iri
            .as_str()
            .starts_with("https://picloud.local/data-domains/"),
        "data domain IRI must be dereferenceable under /data-domains/"
    );
    assert_eq!(
        domain_iri.as_str(),
        "https://picloud.local/data-domains/geospatial",
        "data domain IRI must match cluster-scoped pattern"
    );
    assert!(
        !domain_iri.as_str().contains("/products/"),
        "data domain IRI must not be product-scoped"
    );

    // --- TC-208: Duplicate declaration with the same name is rejected ---
    // The uniqueness guard returns ResourceAlreadyExists before the duplicate
    // projection is applied. Names are cluster-unique (ADR-056).
    assert!(
        projector.data_domain_exists("geospatial").unwrap(),
        "geospatial domain should be discoverable after declaration"
    );
    let duplicate_check = projector.validate_data_domain_unique("geospatial");
    assert!(
        duplicate_check.is_err(),
        "a second data-domain with the same name must be rejected as a duplicate"
    );
    let err = duplicate_check.unwrap_err();
    let err_msg = format!("{err}");
    assert!(
        err_msg.contains("already exists")
            || err_msg.contains("data-domains/geospatial"),
        "duplicate rejection error should mention existing resource: {err_msg}"
    );

    // A brand-new name must still validate as unique.
    assert!(
        projector.validate_data_domain_unique("finance").is_ok(),
        "a different name should pass the uniqueness check"
    );

    // --- TC-208: Deletion guard — cannot delete while data product assigned ---
    // Declare a data product that belongs to 'geospatial'.
    let dp_event = make_data_product_declared(
        "photo-locations",
        "photo-app",
        "geospatial",
        "1.0.0",
    );
    projector.project(&dp_event).await.unwrap();

    // Sanity: the data product exists and is linked to geospatial.
    let member_count = projector
        .count_data_domain_members("geospatial")
        .unwrap();
    assert_eq!(
        member_count, 1,
        "photo-locations should be a member of the geospatial domain"
    );

    // Attempt to delete the domain — must be rejected with a member count error.
    let guard_result = projector.validate_data_domain_deletion("geospatial");
    assert!(
        guard_result.is_err(),
        "data-domain deletion must be rejected while a data product is assigned"
    );
    let guard_err = format!("{}", guard_result.unwrap_err());
    assert!(
        guard_err.contains("1")
            && (guard_err.contains("data product") || guard_err.contains("assigned")),
        "deletion guard should report the member count: {guard_err}"
    );
}

// ============================================================================
// TC-205 — data_domain_deletion_guard
// ============================================================================
/// Attempt to delete `data-domain 'geospatial'` while
/// `photo-app/photo-locations` is assigned to it. Assert the delete is
/// rejected with a member count error (`DataDomainDeletionBlocked`).
#[tokio::test]
async fn data_domain_deletion_guard() {
    let projector = OxigraphProjector::new().unwrap();

    // --- Setup: declare the domain and an assigned data product ---
    let domain = make_data_domain_declared(
        "geospatial",
        "https://picloud.local/platform/identities/alice",
        "internal",
    );
    projector.project(&domain).await.unwrap();

    let dp = make_data_product_declared(
        "photo-locations",
        "photo-app",
        "geospatial",
        "1.0.0",
    );
    projector.project(&dp).await.unwrap();

    // --- Pre-check: domain has exactly one member ---
    let members = projector.count_data_domain_members("geospatial").unwrap();
    assert_eq!(
        members, 1,
        "geospatial should have photo-locations assigned to it"
    );

    // --- Attempt deletion — must be rejected with a member count error ---
    let result = projector.validate_data_domain_deletion("geospatial");
    assert!(
        result.is_err(),
        "delete must be rejected while data products are assigned"
    );

    // The error should be the typed DataDomainDeletionBlocked variant.
    match result.unwrap_err() {
        picloud_domain::error::PiCloudError::DataDomainDeletionBlocked { domain, members } => {
            assert_eq!(domain, "geospatial", "blocked error names the domain");
            assert_eq!(members, 1, "blocked error reports the member count");
        }
        other => panic!(
            "expected DataDomainDeletionBlocked, got: {other:?}"
        ),
    }

    // --- Remove the data product, then deletion is allowed ---
    let dp_iri = {
        let ib = iri_builder();
        format!(
            "{}/data-products/photo-locations",
            ib.product("photo-app").as_str().trim_end_matches('/')
        )
    };
    let dp_delete = make_event(
        "DataProductDeleted",
        Some("photo-app"),
        serde_json::json!({
            "data_product_iri": dp_iri,
            "name": "photo-locations",
            "product": "photo-app",
        }),
    );
    projector.project(&dp_delete).await.unwrap();

    let remaining = projector.count_data_domain_members("geospatial").unwrap();
    assert_eq!(
        remaining, 0,
        "after removing the data product, domain has no members"
    );

    // Now the domain can be deleted without the guard complaining.
    projector
        .validate_data_domain_deletion("geospatial")
        .expect("deletion guard should pass once the domain has no members");
}
