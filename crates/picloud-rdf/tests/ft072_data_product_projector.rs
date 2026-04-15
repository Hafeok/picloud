/// FT-072 Integration Tests — DataProductProjector — cluster RDF graph reflects
/// all data products, domains, producers, consumers, freshness
///
/// Covers:
///   TC-276: DataProductProjector reflects data products and domains in RDF graph (scenario)
///   TC-333: Data projector exit — RDF graph reflects data products and domains (exit-criteria)
///
/// Verifies that the DataProductProjector maintains a complete, consistent
/// RDF graph that reflects:
///   - All declared data products with their metadata
///   - Data domains and their governance attributes
///   - Producer links (data product → owning product via pc:producedBy)
///   - Domain membership links (data product → domain via pc:belongsToDomain)
///   - Freshness metadata (lastRefreshed, tripleCount) after DataProductRefreshed
///   - Cross-product and cross-domain discoverability via SPARQL

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

fn make_data_domain_declared(
    name: &str,
    steward: &str,
    sensitivity: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let domain_iri = ib.cluster_resource("data-domains", name);
    make_event(
        "DataDomainDeclared",
        ResourceIri::new("https://picloud.local/test").unwrap(),
        None, // cluster-scoped, no product
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
        ResourceIri::new(&dp_iri_str).unwrap(),
        Some(product),
        payload,
    )
}

fn make_data_product_refreshed(
    product: &str,
    dp_name: &str,
    triple_count: u64,
    duration_ms: u64,
    trigger_event: &str,
    refreshed_at: &str,
) -> EventEnvelope {
    let dp_iri_str = dp_resource_iri(product, dp_name);
    make_event(
        "DataProductRefreshed",
        ResourceIri::new(&dp_iri_str).unwrap(),
        Some(product),
        serde_json::json!({
            "data_product_iri": dp_iri_str,
            "triple_count": triple_count,
            "duration_ms": duration_ms,
            "trigger_event": trigger_event,
            "refreshed_at": refreshed_at,
        }),
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

fn make_data_domain_deleted(name: &str) -> EventEnvelope {
    let ib = iri_builder();
    let domain_iri = ib.cluster_resource("data-domains", name);
    make_event(
        "DataDomainDeleted",
        ResourceIri::new("https://picloud.local/test").unwrap(),
        None,
        serde_json::json!({
            "domain_iri": domain_iri.as_str(),
            "name": name,
        }),
    )
}

// ============================================================================
// TC-276 — DataProductProjector reflects data products and domains in RDF graph
// ============================================================================
/// Scenario test for FT-072:
///
/// Verifies that the DataProductProjector builds a complete RDF graph
/// reflecting data products, domains, producers, and freshness:
///
/// 1. Declare two data domains (governance, analytics) with stewards
///    and sensitivity. Verify both are discoverable as pc:DataDomain.
///
/// 2. Deploy two products (reporting-app, ml-pipeline). Declare data products
///    in each, linked to different domains. Verify:
///    - Each data product has pc:producedBy → owning product (producer link)
///    - Each data product has pc:belongsToDomain → domain (domain membership)
///    - Metadata (name, version, maxAge) is projected correctly
///
/// 3. Emit DataProductRefreshed for one data product. Verify freshness
///    metadata: pc:lastRefreshed (xsd:dateTime) and pc:tripleCount (xsd:unsignedLong)
///    are present; status transitions to "ready".
///
/// 4. Emit a second DataProductRefreshed. Verify the old freshness values
///    are replaced — only the latest refresh metadata remains.
///
/// 5. Verify cross-product, cross-domain SPARQL discoverability: a single
///    query finds all data products across both products and both domains.
#[tokio::test]
async fn tc276_dataproductprojector_reflects_data_products_and_domains_in_rdf_graph() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // ---- Step 1: Declare data domains ----
    let domain_gov = make_data_domain_declared(
        "governance",
        "https://picloud.local/identities/steward-compliance",
        "restricted",
    );
    let domain_analytics = make_data_domain_declared(
        "analytics",
        "https://picloud.local/identities/steward-data-eng",
        "internal",
    );
    projector.project(&domain_gov).await.unwrap();
    projector.project(&domain_analytics).await.unwrap();

    // Verify both domains exist
    let select_domains = format!(
        "SELECT ?d ?name WHERE {{ \
         ?d <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> ; \
            <{PICLOUD_NS}name> ?name . \
         }} ORDER BY ?name"
    );
    let result = projector.query(&select_domains).await.unwrap();
    assert_eq!(result.bindings.len(), 2, "two data domains should be discoverable");
    assert_eq!(result.bindings[0]["name"]["value"], "analytics");
    assert_eq!(result.bindings[1]["name"]["value"], "governance");

    // ---- Step 2: Deploy products and declare data products ----
    let deploy_reporting = make_product_deployed("reporting-app", "1.0.0");
    let deploy_ml = make_product_deployed("ml-pipeline", "2.0.0");
    projector.project(&deploy_reporting).await.unwrap();
    projector.project(&deploy_ml).await.unwrap();

    // Data product in reporting-app, governance domain
    let dp_compliance = make_data_product_declared(
        "reporting-app",
        "compliance-report",
        "governance",
        "1.0.0",
        Some("PT30M"),
    );
    projector.project(&dp_compliance).await.unwrap();

    // Data product in ml-pipeline, analytics domain
    let dp_features = make_data_product_declared(
        "ml-pipeline",
        "feature-store",
        "analytics",
        "3.0.0",
        Some("PT5M"),
    );
    projector.project(&dp_features).await.unwrap();

    let dp_compliance_iri = dp_resource_iri("reporting-app", "compliance-report");
    let dp_features_iri = dp_resource_iri("ml-pipeline", "feature-store");

    // Verify both data products exist as pc:DataProduct
    let ask_compliance_type = format!(
        "ASK {{ <{dp_compliance_iri}> <{RDF_TYPE}> <{PICLOUD_NS}DataProduct> }}"
    );
    let result = projector.query(&ask_compliance_type).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "compliance-report should be a DataProduct");

    let ask_features_type = format!(
        "ASK {{ <{dp_features_iri}> <{RDF_TYPE}> <{PICLOUD_NS}DataProduct> }}"
    );
    let result = projector.query(&ask_features_type).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "feature-store should be a DataProduct");

    // Verify producer links (pc:producedBy)
    let reporting_product_iri = ib.product("reporting-app");
    let ask_compliance_producer = format!(
        "ASK {{ <{dp_compliance_iri}> <{PICLOUD_NS}producedBy> <{}> }}",
        reporting_product_iri.as_str()
    );
    let result = projector.query(&ask_compliance_producer).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "compliance-report should be producedBy reporting-app"
    );

    let ml_product_iri = ib.product("ml-pipeline");
    let ask_features_producer = format!(
        "ASK {{ <{dp_features_iri}> <{PICLOUD_NS}producedBy> <{}> }}",
        ml_product_iri.as_str()
    );
    let result = projector.query(&ask_features_producer).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "feature-store should be producedBy ml-pipeline"
    );

    // Verify domain membership links (pc:belongsToDomain)
    let cluster_root = ib.cluster_root();
    let governance_domain_iri = format!(
        "{}/data-domains/governance",
        cluster_root.as_str().trim_end_matches('/')
    );
    let analytics_domain_iri = format!(
        "{}/data-domains/analytics",
        cluster_root.as_str().trim_end_matches('/')
    );

    let ask_compliance_domain = format!(
        "ASK {{ <{dp_compliance_iri}> <{PICLOUD_NS}belongsToDomain> <{governance_domain_iri}> }}"
    );
    let result = projector.query(&ask_compliance_domain).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "compliance-report should belongsToDomain governance"
    );

    let ask_features_domain = format!(
        "ASK {{ <{dp_features_iri}> <{PICLOUD_NS}belongsToDomain> <{analytics_domain_iri}> }}"
    );
    let result = projector.query(&ask_features_domain).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "feature-store should belongsToDomain analytics"
    );

    // Verify metadata: version and maxAge (freshness SLO)
    let ask_compliance_version = format!(
        "ASK {{ <{dp_compliance_iri}> <{PICLOUD_NS}version> \"1.0.0\" }}"
    );
    let result = projector.query(&ask_compliance_version).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "compliance-report version should be 1.0.0");

    let ask_compliance_max_age = format!(
        "ASK {{ <{dp_compliance_iri}> <{PICLOUD_NS}maxAge> \"PT30M\" }}"
    );
    let result = projector.query(&ask_compliance_max_age).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "compliance-report maxAge should be PT30M");

    let ask_features_max_age = format!(
        "ASK {{ <{dp_features_iri}> <{PICLOUD_NS}maxAge> \"PT5M\" }}"
    );
    let result = projector.query(&ask_features_max_age).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "feature-store maxAge should be PT5M");

    // Verify initial status is "declared" for both
    let ask_compliance_status = format!(
        "ASK {{ <{dp_compliance_iri}> <{PICLOUD_NS}status> \"declared\" }}"
    );
    let result = projector.query(&ask_compliance_status).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "compliance-report status should be declared");

    let ask_features_status = format!(
        "ASK {{ <{dp_features_iri}> <{PICLOUD_NS}status> \"declared\" }}"
    );
    let result = projector.query(&ask_features_status).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "feature-store status should be declared");

    // ---- Step 3: DataProductRefreshed — freshness metadata ----
    let refreshed_ts = "2026-04-15T10:30:00Z";
    let refreshed = make_data_product_refreshed(
        "ml-pipeline",
        "feature-store",
        42_000,
        350,
        "FeatureIngested",
        refreshed_ts,
    );
    projector.project(&refreshed).await.unwrap();

    // Status should transition to "ready" — update_status stores the label
    // on pc:statusLabel (literal) and the NamedNode on pc:status (ADR convention)
    let ask_features_ready = format!(
        "ASK {{ <{dp_features_iri}> <{PICLOUD_NS}statusLabel> \"ready\" }}"
    );
    let result = projector.query(&ask_features_ready).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "feature-store statusLabel should be 'ready' after DataProductRefreshed"
    );

    // Also verify the NamedNode form of the status
    let ask_features_ready_nn = format!(
        "ASK {{ <{dp_features_iri}> <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> }}"
    );
    let result = projector.query(&ask_features_ready_nn).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "feature-store status should be pc:Ready NamedNode after DataProductRefreshed"
    );

    // Verify lastRefreshed (xsd:dateTime)
    let ask_last_refreshed = format!(
        "PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> \
         ASK {{ <{dp_features_iri}> <{PICLOUD_NS}lastRefreshed> \"{refreshed_ts}\"^^xsd:dateTime }}"
    );
    let result = projector.query(&ask_last_refreshed).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "feature-store should have lastRefreshed = 2026-04-15T10:30:00Z"
    );

    // Verify tripleCount (xsd:unsignedLong)
    let ask_triple_count = format!(
        "PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> \
         ASK {{ <{dp_features_iri}> <{PICLOUD_NS}tripleCount> \"42000\"^^xsd:unsignedLong }}"
    );
    let result = projector.query(&ask_triple_count).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "feature-store should have tripleCount = 42000"
    );

    // ---- Step 4: Second refresh — only latest values remain ----
    let refreshed_ts_2 = "2026-04-15T11:00:00Z";
    let refreshed_2 = make_data_product_refreshed(
        "ml-pipeline",
        "feature-store",
        45_000,
        280,
        "FeatureIngested",
        refreshed_ts_2,
    );
    projector.project(&refreshed_2).await.unwrap();

    // Old lastRefreshed should be gone
    let ask_old_refreshed = format!(
        "PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> \
         ASK {{ <{dp_features_iri}> <{PICLOUD_NS}lastRefreshed> \"{refreshed_ts}\"^^xsd:dateTime }}"
    );
    let result = projector.query(&ask_old_refreshed).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "old lastRefreshed should be replaced after second refresh"
    );

    // New lastRefreshed should be present
    let ask_new_refreshed = format!(
        "PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> \
         ASK {{ <{dp_features_iri}> <{PICLOUD_NS}lastRefreshed> \"{refreshed_ts_2}\"^^xsd:dateTime }}"
    );
    let result = projector.query(&ask_new_refreshed).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "new lastRefreshed should be present after second refresh"
    );

    // Verify exactly one lastRefreshed triple
    let select_refreshed = format!(
        "SELECT ?ts WHERE {{ <{dp_features_iri}> <{PICLOUD_NS}lastRefreshed> ?ts }}"
    );
    let result = projector.query(&select_refreshed).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        1,
        "exactly one lastRefreshed triple should exist (no stale duplicates)"
    );

    // New tripleCount should be present
    let ask_new_triple_count = format!(
        "PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> \
         ASK {{ <{dp_features_iri}> <{PICLOUD_NS}tripleCount> \"45000\"^^xsd:unsignedLong }}"
    );
    let result = projector.query(&ask_new_triple_count).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "tripleCount should be updated to 45000 after second refresh"
    );

    // ---- Step 5: Cross-product, cross-domain SPARQL discoverability ----
    // Find all data products across all products and domains
    let select_all_dps = format!(
        "SELECT ?dp ?name ?product ?domain WHERE {{ \
         ?dp <{RDF_TYPE}> <{PICLOUD_NS}DataProduct> ; \
             <{PICLOUD_NS}name> ?name ; \
             <{PICLOUD_NS}product> ?product ; \
             <{PICLOUD_NS}domain> ?domain . \
         }} ORDER BY ?name"
    );
    let result = projector.query(&select_all_dps).await.unwrap();
    assert_eq!(result.bindings.len(), 2, "both data products should be discoverable");
    assert_eq!(result.bindings[0]["name"]["value"], "compliance-report");
    assert_eq!(result.bindings[0]["product"]["value"], "reporting-app");
    assert_eq!(result.bindings[0]["domain"]["value"], "governance");
    assert_eq!(result.bindings[1]["name"]["value"], "feature-store");
    assert_eq!(result.bindings[1]["product"]["value"], "ml-pipeline");
    assert_eq!(result.bindings[1]["domain"]["value"], "analytics");

    // Find all data products produced by a specific product
    let select_by_producer = format!(
        "SELECT ?dp ?name WHERE {{ \
         ?dp <{PICLOUD_NS}producedBy> <{}> ; \
             <{PICLOUD_NS}name> ?name . \
         }}",
        ml_product_iri.as_str()
    );
    let result = projector.query(&select_by_producer).await.unwrap();
    assert_eq!(result.bindings.len(), 1, "one data product produced by ml-pipeline");
    assert_eq!(result.bindings[0]["name"]["value"], "feature-store");

    // Find all data products belonging to a domain
    let select_by_domain = format!(
        "SELECT ?dp ?name WHERE {{ \
         ?dp <{PICLOUD_NS}belongsToDomain> <{governance_domain_iri}> ; \
             <{PICLOUD_NS}name> ?name . \
         }}"
    );
    let result = projector.query(&select_by_domain).await.unwrap();
    assert_eq!(result.bindings.len(), 1, "one data product in governance domain");
    assert_eq!(result.bindings[0]["name"]["value"], "compliance-report");
}

// ============================================================================
// TC-333 — Data projector exit — RDF graph reflects data products and domains
// ============================================================================
/// Exit-criteria test for FT-072:
///
/// Verifies the complete invariant: the RDF graph correctly reflects the full
/// lifecycle of data products and domains together. This is the gate criterion —
/// if this passes, the feature is considered complete.
///
/// Exercises:
///   - Three products, each with data products across overlapping domains
///   - Domain creation, data product creation, freshness refresh
///   - Selective deletion of data products and domains
///   - Graph consistency after partial deletions
///   - Discovery queries that span the entire cluster graph
#[tokio::test]
async fn tc333_data_projector_exit_rdf_graph_reflects_data_products_and_domains() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // ---- Setup: Three domains ----
    let dom_fin = make_data_domain_declared(
        "finance",
        "https://picloud.local/identities/steward-cfo",
        "restricted",
    );
    let dom_eng = make_data_domain_declared(
        "engineering",
        "https://picloud.local/identities/steward-vpe",
        "internal",
    );
    let dom_ops = make_data_domain_declared(
        "operations",
        "https://picloud.local/identities/steward-coo",
        "confidential",
    );
    projector.project(&dom_fin).await.unwrap();
    projector.project(&dom_eng).await.unwrap();
    projector.project(&dom_ops).await.unwrap();

    // Verify three domains discoverable
    let select_domains = format!(
        "SELECT ?d WHERE {{ ?d <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> }}"
    );
    let result = projector.query(&select_domains).await.unwrap();
    assert_eq!(result.bindings.len(), 3, "three data domains should exist");

    // ---- Setup: Three products ----
    projector.project(&make_product_deployed("billing-svc", "1.0.0")).await.unwrap();
    projector.project(&make_product_deployed("ci-platform", "2.0.0")).await.unwrap();
    projector.project(&make_product_deployed("monitoring-hub", "1.5.0")).await.unwrap();

    // ---- Declare data products across products and domains ----
    // billing-svc has two data products: invoices (finance), cost-allocation (operations)
    let dp_invoices = make_data_product_declared(
        "billing-svc", "invoices", "finance", "1.0.0", Some("PT1H"),
    );
    let dp_costs = make_data_product_declared(
        "billing-svc", "cost-allocation", "operations", "1.0.0", Some("PT4H"),
    );

    // ci-platform has one data product: build-metrics (engineering)
    let dp_builds = make_data_product_declared(
        "ci-platform", "build-metrics", "engineering", "2.0.0", Some("PT10M"),
    );

    // monitoring-hub has two data products: slo-dashboard (operations), incident-log (engineering)
    let dp_slo = make_data_product_declared(
        "monitoring-hub", "slo-dashboard", "operations", "1.0.0", Some("PT5M"),
    );
    let dp_incidents = make_data_product_declared(
        "monitoring-hub", "incident-log", "engineering", "1.0.0", None,
    );

    projector.project(&dp_invoices).await.unwrap();
    projector.project(&dp_costs).await.unwrap();
    projector.project(&dp_builds).await.unwrap();
    projector.project(&dp_slo).await.unwrap();
    projector.project(&dp_incidents).await.unwrap();

    // ---- Verify: All five data products are discoverable ----
    let select_all_dps = format!(
        "SELECT ?dp ?name ?product ?domain WHERE {{ \
         ?dp <{RDF_TYPE}> <{PICLOUD_NS}DataProduct> ; \
             <{PICLOUD_NS}name> ?name ; \
             <{PICLOUD_NS}product> ?product ; \
             <{PICLOUD_NS}domain> ?domain . \
         }} ORDER BY ?name"
    );
    let result = projector.query(&select_all_dps).await.unwrap();
    assert_eq!(result.bindings.len(), 5, "five data products should be discoverable");

    // Verify expected names in sorted order
    let names: Vec<&str> = result.bindings.iter()
        .map(|b| b["name"]["value"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["build-metrics", "cost-allocation", "incident-log", "invoices", "slo-dashboard"],
        "data product names should match in sorted order"
    );

    // ---- Verify: Producer links for all products ----
    let billing_iri = ib.product("billing-svc");
    let select_billing_dps = format!(
        "SELECT ?name WHERE {{ \
         ?dp <{PICLOUD_NS}producedBy> <{}> ; \
             <{PICLOUD_NS}name> ?name . \
         }} ORDER BY ?name",
        billing_iri.as_str()
    );
    let result = projector.query(&select_billing_dps).await.unwrap();
    assert_eq!(result.bindings.len(), 2, "billing-svc should produce two data products");
    let billing_names: Vec<&str> = result.bindings.iter()
        .map(|b| b["name"]["value"].as_str().unwrap())
        .collect();
    assert_eq!(billing_names, vec!["cost-allocation", "invoices"]);

    let monitoring_iri = ib.product("monitoring-hub");
    let select_monitoring_dps = format!(
        "SELECT ?name WHERE {{ \
         ?dp <{PICLOUD_NS}producedBy> <{}> ; \
             <{PICLOUD_NS}name> ?name . \
         }} ORDER BY ?name",
        monitoring_iri.as_str()
    );
    let result = projector.query(&select_monitoring_dps).await.unwrap();
    assert_eq!(result.bindings.len(), 2, "monitoring-hub should produce two data products");
    let monitoring_names: Vec<&str> = result.bindings.iter()
        .map(|b| b["name"]["value"].as_str().unwrap())
        .collect();
    assert_eq!(monitoring_names, vec!["incident-log", "slo-dashboard"]);

    // ---- Verify: Domain membership links ----
    let cluster_root = ib.cluster_root();
    let ops_domain_iri = format!(
        "{}/data-domains/operations",
        cluster_root.as_str().trim_end_matches('/')
    );
    let eng_domain_iri = format!(
        "{}/data-domains/engineering",
        cluster_root.as_str().trim_end_matches('/')
    );

    let select_ops_dps = format!(
        "SELECT ?name WHERE {{ \
         ?dp <{PICLOUD_NS}belongsToDomain> <{ops_domain_iri}> ; \
             <{PICLOUD_NS}name> ?name . \
         }} ORDER BY ?name"
    );
    let result = projector.query(&select_ops_dps).await.unwrap();
    assert_eq!(result.bindings.len(), 2, "operations domain should have two data products");
    let ops_names: Vec<&str> = result.bindings.iter()
        .map(|b| b["name"]["value"].as_str().unwrap())
        .collect();
    assert_eq!(ops_names, vec!["cost-allocation", "slo-dashboard"]);

    let select_eng_dps = format!(
        "SELECT ?name WHERE {{ \
         ?dp <{PICLOUD_NS}belongsToDomain> <{eng_domain_iri}> ; \
             <{PICLOUD_NS}name> ?name . \
         }} ORDER BY ?name"
    );
    let result = projector.query(&select_eng_dps).await.unwrap();
    assert_eq!(result.bindings.len(), 2, "engineering domain should have two data products");
    let eng_names: Vec<&str> = result.bindings.iter()
        .map(|b| b["name"]["value"].as_str().unwrap())
        .collect();
    assert_eq!(eng_names, vec!["build-metrics", "incident-log"]);

    // ---- Verify: Freshness SLOs ----
    let select_with_slo = format!(
        "SELECT ?name ?slo WHERE {{ \
         ?dp <{RDF_TYPE}> <{PICLOUD_NS}DataProduct> ; \
             <{PICLOUD_NS}name> ?name ; \
             <{PICLOUD_NS}maxAge> ?slo . \
         }} ORDER BY ?name"
    );
    let result = projector.query(&select_with_slo).await.unwrap();
    // incident-log has no maxAge, so only 4 results
    assert_eq!(result.bindings.len(), 4, "four data products should have a freshness SLO");
    let slo_names: Vec<&str> = result.bindings.iter()
        .map(|b| b["name"]["value"].as_str().unwrap())
        .collect();
    assert_eq!(
        slo_names,
        vec!["build-metrics", "cost-allocation", "invoices", "slo-dashboard"],
        "data products with SLOs should be discoverable"
    );

    // ---- Refresh two data products — verify freshness metadata ----
    let refresh_builds = make_data_product_refreshed(
        "ci-platform", "build-metrics", 10_000, 120, "BuildCompleted", "2026-04-15T09:00:00Z",
    );
    let refresh_slo = make_data_product_refreshed(
        "monitoring-hub", "slo-dashboard", 5_000, 80, "MetricIngested", "2026-04-15T09:05:00Z",
    );
    projector.project(&refresh_builds).await.unwrap();
    projector.project(&refresh_slo).await.unwrap();

    let dp_builds_iri = dp_resource_iri("ci-platform", "build-metrics");
    let dp_slo_iri = dp_resource_iri("monitoring-hub", "slo-dashboard");

    // Both should be status "ready" now — update_status stores literal on pc:statusLabel
    let ask_builds_ready = format!(
        "ASK {{ <{dp_builds_iri}> <{PICLOUD_NS}statusLabel> \"ready\" }}"
    );
    let result = projector.query(&ask_builds_ready).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "build-metrics should be ready after refresh");

    let ask_slo_ready = format!(
        "ASK {{ <{dp_slo_iri}> <{PICLOUD_NS}statusLabel> \"ready\" }}"
    );
    let result = projector.query(&ask_slo_ready).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "slo-dashboard should be ready after refresh");

    // Verify freshness metadata
    let ask_builds_refreshed = format!(
        "PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> \
         ASK {{ <{dp_builds_iri}> <{PICLOUD_NS}lastRefreshed> \"2026-04-15T09:00:00Z\"^^xsd:dateTime }}"
    );
    let result = projector.query(&ask_builds_refreshed).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "build-metrics lastRefreshed should be set");

    let ask_builds_count = format!(
        "PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> \
         ASK {{ <{dp_builds_iri}> <{PICLOUD_NS}tripleCount> \"10000\"^^xsd:unsignedLong }}"
    );
    let result = projector.query(&ask_builds_count).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "build-metrics tripleCount should be 10000");

    // Un-refreshed data products should still be "declared" status
    let dp_invoices_iri = dp_resource_iri("billing-svc", "invoices");
    let ask_invoices_declared = format!(
        "ASK {{ <{dp_invoices_iri}> <{PICLOUD_NS}status> \"declared\" }}"
    );
    let result = projector.query(&ask_invoices_declared).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "un-refreshed data products should remain in 'declared' status"
    );

    // ---- Update a data product — domain reassignment ----
    let update_costs = make_data_product_updated(
        "billing-svc", "cost-allocation", "finance", "1.1.0", Some("PT2H"),
        Some("Moved from operations to finance domain"),
    );
    projector.project(&update_costs).await.unwrap();

    let dp_costs_iri = dp_resource_iri("billing-svc", "cost-allocation");

    // cost-allocation should now belong to finance domain
    let fin_domain_iri = format!(
        "{}/data-domains/finance",
        cluster_root.as_str().trim_end_matches('/')
    );
    let ask_costs_fin = format!(
        "ASK {{ <{dp_costs_iri}> <{PICLOUD_NS}belongsToDomain> <{fin_domain_iri}> }}"
    );
    let result = projector.query(&ask_costs_fin).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "cost-allocation should now belong to finance domain"
    );

    // Should no longer belong to operations
    let ask_costs_ops = format!(
        "ASK {{ <{dp_costs_iri}> <{PICLOUD_NS}belongsToDomain> <{ops_domain_iri}> }}"
    );
    let result = projector.query(&ask_costs_ops).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "cost-allocation should no longer belong to operations domain"
    );

    // Operations domain should now have only 1 data product (slo-dashboard)
    let result = projector.query(&select_ops_dps).await.unwrap();
    assert_eq!(
        result.bindings.len(), 1,
        "operations domain should have one data product after reassignment"
    );
    assert_eq!(result.bindings[0]["name"]["value"], "slo-dashboard");

    // Finance domain should now have 2 data products (invoices + cost-allocation)
    let select_fin_dps = format!(
        "SELECT ?name WHERE {{ \
         ?dp <{PICLOUD_NS}belongsToDomain> <{fin_domain_iri}> ; \
             <{PICLOUD_NS}name> ?name . \
         }} ORDER BY ?name"
    );
    let result = projector.query(&select_fin_dps).await.unwrap();
    assert_eq!(result.bindings.len(), 2, "finance domain should have two data products");
    let fin_names: Vec<&str> = result.bindings.iter()
        .map(|b| b["name"]["value"].as_str().unwrap())
        .collect();
    assert_eq!(fin_names, vec!["cost-allocation", "invoices"]);

    // ---- Delete one data product — graph consistency ----
    let delete_incidents = make_data_product_deleted("monitoring-hub", "incident-log");
    projector.project(&delete_incidents).await.unwrap();

    // Total data products should be 4
    let result = projector.query(&select_all_dps).await.unwrap();
    assert_eq!(
        result.bindings.len(), 4,
        "four data products should remain after deleting incident-log"
    );

    // engineering domain should now have only build-metrics
    let result = projector.query(&select_eng_dps).await.unwrap();
    assert_eq!(
        result.bindings.len(), 1,
        "engineering domain should have one data product after deletion"
    );
    assert_eq!(result.bindings[0]["name"]["value"], "build-metrics");

    // monitoring-hub should now produce only slo-dashboard
    let result = projector.query(&select_monitoring_dps).await.unwrap();
    assert_eq!(
        result.bindings.len(), 1,
        "monitoring-hub should produce one data product after deletion"
    );
    assert_eq!(result.bindings[0]["name"]["value"], "slo-dashboard");

    // Deleted data product should have no triples
    let dp_incidents_iri = dp_resource_iri("monitoring-hub", "incident-log");
    let select_deleted = format!(
        "SELECT ?p ?o WHERE {{ <{dp_incidents_iri}> ?p ?o }}"
    );
    let result = projector.query(&select_deleted).await.unwrap();
    assert_eq!(
        result.bindings.len(), 0,
        "no triples should remain for deleted incident-log"
    );

    // ---- Delete a domain — domain metadata removed, data products survive ----
    let delete_ops_domain = make_data_domain_deleted("operations");
    projector.project(&delete_ops_domain).await.unwrap();

    // Domain should be gone
    let result = projector.query(&select_domains).await.unwrap();
    assert_eq!(result.bindings.len(), 2, "two domains should remain after deleting operations");

    // slo-dashboard still exists and still has its belongsToDomain link
    // (the link points to the domain IRI, which no longer has metadata —
    // this is valid: the data product retains its declared domain affiliation)
    let ask_slo_exists = format!(
        "ASK {{ <{dp_slo_iri}> <{RDF_TYPE}> <{PICLOUD_NS}DataProduct> }}"
    );
    let result = projector.query(&ask_slo_exists).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "slo-dashboard should still exist after its domain is deleted"
    );

    // ---- Final: Delete remaining data products and verify empty ----
    let remaining_dps = vec![
        ("billing-svc", "invoices"),
        ("billing-svc", "cost-allocation"),
        ("ci-platform", "build-metrics"),
        ("monitoring-hub", "slo-dashboard"),
    ];
    for (product, dp_name) in &remaining_dps {
        let delete = make_data_product_deleted(product, dp_name);
        projector.project(&delete).await.unwrap();
    }

    // No data products should remain
    let result = projector.query(&select_all_dps).await.unwrap();
    assert_eq!(
        result.bindings.len(), 0,
        "no data products should remain after deleting all"
    );

    // Clean up remaining domains
    let delete_fin = make_data_domain_deleted("finance");
    let delete_eng = make_data_domain_deleted("engineering");
    projector.project(&delete_fin).await.unwrap();
    projector.project(&delete_eng).await.unwrap();

    // No domains should remain
    let result = projector.query(&select_domains).await.unwrap();
    assert_eq!(
        result.bindings.len(), 0,
        "no data domains should remain after deleting all"
    );
}
