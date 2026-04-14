/// FT-031 Integration Tests — Cascading Deletion
///
/// Covers TC-245 and TC-302.
/// These tests verify that deleting a Product cascades to all child resources
/// (containers, volumes, identities) both in the default graph and in the
/// product's named graph.

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::StateProjector;
use picloud_rdf::OxigraphProjector;
use uuid::Uuid;

const PICLOUD_NS: &str = "https://picloud.local/ontology#";

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

fn make_product_deployed(product_name: &str, version: &str) -> EventEnvelope {
    let ib = iri_builder();
    let product_iri = ib.product(product_name);
    make_event(
        "ProductDeployed",
        product_iri.clone(),
        Some(product_name),
        serde_json::json!({
            "product_iri": product_iri.as_str(),
            "product_name": product_name,
            "version": version,
        }),
    )
}

fn make_resource_declared(
    product: &str,
    resource_type: &str,
    name: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let resource_iri = ib.resource(product, &resource_type.to_lowercase().replace(' ', "-"), name);
    make_event(
        "ResourceDeclared",
        resource_iri.clone(),
        Some(product),
        serde_json::json!({
            "resource_iri": resource_iri.as_str(),
            "resource_type": resource_type,
            "product": product,
            "name": name,
        }),
    )
}

fn make_product_deleted(product_name: &str) -> EventEnvelope {
    let ib = iri_builder();
    let product_iri = ib.product(product_name);
    make_event(
        "ProductDeleted",
        product_iri.clone(),
        Some(product_name),
        serde_json::json!({
            "product_iri": product_iri.as_str(),
        }),
    )
}

/// Helper: project all events into a projector and return the projector.
async fn project_events(events: &[EventEnvelope]) -> OxigraphProjector {
    let projector = OxigraphProjector::new().unwrap();
    for event in events {
        projector.project(event).await.unwrap();
    }
    projector
}

/// Helper: query and assert specific count
async fn assert_sparql_count(projector: &OxigraphProjector, sparql: &str, expected: usize) {
    let result = projector.query(sparql).await.unwrap();
    assert_eq!(
        result.bindings.len(),
        expected,
        "Expected {expected} results for SPARQL: {sparql}, got {}: {:?}",
        result.bindings.len(),
        result.bindings
    );
}

// ============================================================================
// TC-245 — Delete Product cascades to all child containers, volumes, and
//           identities
// ============================================================================
/// Deploy a product with containers, volumes, and workload identities.
/// Delete the product. Verify that every child resource is removed from both
/// the default graph and the product's named graph.
#[tokio::test]
async fn tc245_delete_product_cascades_to_all_child_containers_volumes_and_identities() {
    let product = "cascade-app";
    let ib = iri_builder();

    // 1. Deploy product and declare child resources of various types
    let events = vec![
        make_product_deployed(product, "1.0.0"),
        make_resource_declared(product, "Container", "api-server"),
        make_resource_declared(product, "Container", "worker"),
        make_resource_declared(product, "Volume", "data-store"),
        make_resource_declared(product, "Volume", "cache"),
        // Workload identities are product-scoped resources
        make_resource_declared(product, "Identity", "service-account"),
        make_resource_declared(product, "Identity", "deploy-bot"),
    ];

    let projector = project_events(&events).await;

    // 2. Verify all resources exist in the default graph
    let count_query = format!(
        "SELECT ?res WHERE {{ ?res <{PICLOUD_NS}product> \"{product}\" }}"
    );
    assert_sparql_count(&projector, &count_query, 6).await;

    // 3. Verify the product itself exists
    let product_iri = ib.product(product);
    let product_query = format!(
        "SELECT ?type WHERE {{ <{}> <{PICLOUD_NS}resourceType> ?type }}",
        product_iri.as_str()
    );
    assert_sparql_count(&projector, &product_query, 1).await;

    // 4. Verify resources exist in the product's named graph
    let named_result = projector.query_product(&product_iri, &format!("?res <{PICLOUD_NS}resourceType> ?t")).await.unwrap();
    assert_eq!(named_result.bindings.len(), 6, "6 child resources in named graph before delete");

    // 5. Delete the product
    let delete_event = make_product_deleted(product);
    projector.project(&delete_event).await.unwrap();

    // 6. Verify the product is gone from the default graph
    assert_sparql_count(&projector, &product_query, 0).await;

    // 7. Verify ALL child resources are gone from the default graph
    assert_sparql_count(&projector, &count_query, 0).await;

    // 8. Verify child resources are gone from the named graph
    let named_after = projector.query_product(&product_iri, &format!("?res <{PICLOUD_NS}resourceType> ?t")).await.unwrap();
    assert_eq!(named_after.bindings.len(), 0, "No child resources in named graph after delete");

    // 9. Verify specific resource types are individually gone
    let container_iri = ib.resource(product, "container", "api-server");
    let container_query = format!(
        "SELECT ?p ?o WHERE {{ <{}> ?p ?o }}", container_iri.as_str()
    );
    assert_sparql_count(&projector, &container_query, 0).await;

    let volume_iri = ib.resource(product, "volume", "data-store");
    let volume_query = format!(
        "SELECT ?p ?o WHERE {{ <{}> ?p ?o }}", volume_iri.as_str()
    );
    assert_sparql_count(&projector, &volume_query, 0).await;

    let identity_iri = ib.resource(product, "identity", "service-account");
    let identity_query = format!(
        "SELECT ?p ?o WHERE {{ <{}> ?p ?o }}", identity_iri.as_str()
    );
    assert_sparql_count(&projector, &identity_query, 0).await;
}

// ============================================================================
// TC-302 — Cascading delete exit — product deletion removes all child resources
// ============================================================================
/// Exit criterion: after product deletion, zero resources reference the deleted
/// product anywhere in the graph. Also verifies that sibling products and their
/// resources are unaffected.
#[tokio::test]
async fn tc302_cascading_delete_exit_product_deletion_removes_all_child_resources() {
    let target = "doomed-product";
    let sibling = "safe-product";
    let ib = iri_builder();

    // 1. Deploy two products, each with child resources
    let events = vec![
        // Target product (will be deleted)
        make_product_deployed(target, "1.0.0"),
        make_resource_declared(target, "Container", "web"),
        make_resource_declared(target, "Container", "api"),
        make_resource_declared(target, "Volume", "db-data"),
        make_resource_declared(target, "Identity", "svc-id"),
        // Sibling product (must survive)
        make_product_deployed(sibling, "2.0.0"),
        make_resource_declared(sibling, "Container", "frontend"),
        make_resource_declared(sibling, "Volume", "assets"),
    ];

    let projector = project_events(&events).await;

    // 2. Verify both products exist
    let all_products_query = format!(
        "SELECT ?p WHERE {{ ?p <{PICLOUD_NS}resourceType> \"Product\" }}"
    );
    assert_sparql_count(&projector, &all_products_query, 2).await;

    // 3. Delete the target product
    let delete_event = make_product_deleted(target);
    projector.project(&delete_event).await.unwrap();

    // 4. EXIT CRITERION: zero resources reference the deleted product
    let target_refs_query = format!(
        "SELECT ?res WHERE {{ ?res <{PICLOUD_NS}product> \"{target}\" }}"
    );
    assert_sparql_count(&projector, &target_refs_query, 0).await;

    // 5. EXIT CRITERION: the product itself is gone
    let target_iri = ib.product(target);
    let target_query = format!(
        "SELECT ?p ?o WHERE {{ <{}> ?p ?o }}", target_iri.as_str()
    );
    assert_sparql_count(&projector, &target_query, 0).await;

    // 6. EXIT CRITERION: the product's named graph is empty
    let named_result = projector.query_product(
        &target_iri,
        &format!("?s ?p ?o"),
    ).await.unwrap();
    assert_eq!(named_result.bindings.len(), 0, "Named graph must be empty after product deletion");

    // 7. EXIT CRITERION: no orphaned triples with product IRI path
    let orphan_query = format!(
        "SELECT ?s WHERE {{ ?s ?p ?o . FILTER(CONTAINS(STR(?s), \"/products/{target}/\")) }}"
    );
    assert_sparql_count(&projector, &orphan_query, 0).await;

    // 8. ISOLATION: sibling product and its resources survive
    let sibling_iri = ib.product(sibling);
    let sibling_query = format!(
        "SELECT ?type WHERE {{ <{}> <{PICLOUD_NS}resourceType> ?type }}",
        sibling_iri.as_str()
    );
    assert_sparql_count(&projector, &sibling_query, 1).await;

    let sibling_children_query = format!(
        "SELECT ?res WHERE {{ ?res <{PICLOUD_NS}product> \"{sibling}\" }}"
    );
    assert_sparql_count(&projector, &sibling_children_query, 2).await;

    // 9. Only one product should remain
    assert_sparql_count(&projector, &all_products_query, 1).await;
}
