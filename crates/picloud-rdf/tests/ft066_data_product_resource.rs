/// FT-066 Integration Tests — Data Product Resource Type
///
/// Covers:
///   TC-231: Data product declared, projection rebuilt on trigger event,
///           second product queries it (exit-criteria)
///
/// Verifies the full data product lifecycle through RDF projection
/// and the SPARQL CONSTRUCT data product projector:
///   1. Product A (photo-app) is deployed and populates its internal graph
///      with domain triples (e.g., photo locations)
///   2. A data product is declared within product A, referencing a CONSTRUCT
///      query that projects curated data into a separate named graph
///   3. A trigger event causes the projection to rebuild — the data product
///      named graph is atomically swapped with fresh CONSTRUCT results
///   4. Product B (maps-app) queries the data product's published graph
///      and retrieves the projected triples
///   5. A second trigger event updates the source data; re-projection
///      reflects the changes, and old stale data is removed

use std::sync::Arc;

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{DataProductProjector, StateProjector};
use picloud_rdf::{OxigraphDataProductProjector, OxigraphProjector};
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

fn make_data_product_declared(
    product: &str,
    dp_name: &str,
    domain: &str,
    version: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let dp_iri = ib.data_product_graph(product, dp_name);
    // The data product IRI is the graph IRI without "/graph"
    let dp_resource_iri_str = dp_iri.as_str().trim_end_matches("/graph");
    make_event(
        "DataProductDeclared",
        ResourceIri::new(dp_resource_iri_str).unwrap(),
        Some(product),
        serde_json::json!({
            "data_product_iri": dp_resource_iri_str,
            "name": dp_name,
            "product": product,
            "domain": domain,
            "version": version,
        }),
    )
}

fn make_data_product_refreshed(
    product: &str,
    dp_name: &str,
    triple_count: u64,
    trigger_event: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let dp_iri = ib.data_product_graph(product, dp_name);
    let dp_resource_iri_str = dp_iri.as_str().trim_end_matches("/graph");
    make_event(
        "DataProductRefreshed",
        ResourceIri::new(dp_resource_iri_str).unwrap(),
        Some(product),
        serde_json::json!({
            "data_product_iri": dp_resource_iri_str,
            "triple_count": triple_count,
            "duration_ms": 42,
            "trigger_event": trigger_event,
            "refreshed_at": "2026-04-15T10:00:00Z",
        }),
    )
}

// ============================================================================
// TC-231 — Data product declared, projection rebuilt on trigger event,
//          second product queries it
// ============================================================================
/// Exit-criteria test for data-product resource type (FT-066):
///
/// 1. Deploy product A ("photo-app") and seed its internal operational graph
///    with domain triples (photo locations with coordinates).
/// 2. Declare a data product ("photo-locations") scoped to product A, with
///    a SPARQL CONSTRUCT projection query.
/// 3. Simulate a trigger event (e.g. PlaceResolved) by executing
///    `refresh_projection` — verify the data product's own named graph
///    is populated with the CONSTRUCT result.
/// 4. Verify the RDF catalog records the data product as "ready" with
///    correct triple count and metadata.
/// 5. Deploy product B ("maps-app") and query the data product's published
///    named graph — verify product B can read the projected data.
/// 6. Update source data (add a new photo location) and re-trigger the
///    projection — verify the data product graph reflects the update and
///    stale data is removed (atomic swap).
/// 7. Product B re-queries and sees the updated projection.
#[tokio::test]
async fn tc231_data_product_declared_projection_rebuilt_on_trigger_event_second_product_queries_it()
{
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // ---- Step 1: Deploy product A ("photo-app") ----
    let deploy_a = make_product_deployed("photo-app", "1.0.0");
    projector.project(&deploy_a).await.unwrap();

    // Seed product A's internal operational graph with domain triples.
    // In the real platform this would happen via event projections; here we
    // insert directly into the store to simulate an existing operational graph.
    let product_graph_iri = ib.product_graph("photo-app");
    {
        use oxigraph::model::{Literal, NamedNode, NamedNodeRef, QuadRef};

        let store = projector.store();
        store
            .insert_named_graph(NamedNodeRef::new(product_graph_iri.as_str()).unwrap())
            .unwrap();

        let g = NamedNode::new(product_graph_iri.as_str()).unwrap();

        // Photo 1 — at location (48.8566, 2.3522) = Paris
        let photo1 = NamedNode::new("https://picloud.local/products/photo-app/photos/p1").unwrap();
        let type_pred = NamedNode::new(RDF_TYPE).unwrap();
        let photo_type = NamedNode::new(&format!("{PICLOUD_NS}Photo")).unwrap();
        store
            .insert(QuadRef::new(&photo1, &type_pred, &photo_type, &g))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo1,
                &NamedNode::new(&format!("{PICLOUD_NS}placeName")).unwrap(),
                &Literal::new_simple_literal("Paris"),
                &g,
            ))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo1,
                &NamedNode::new(&format!("{PICLOUD_NS}latitude")).unwrap(),
                &Literal::new_simple_literal("48.8566"),
                &g,
            ))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo1,
                &NamedNode::new(&format!("{PICLOUD_NS}longitude")).unwrap(),
                &Literal::new_simple_literal("2.3522"),
                &g,
            ))
            .unwrap();

        // Photo 2 — at location (51.5074, -0.1278) = London
        let photo2 = NamedNode::new("https://picloud.local/products/photo-app/photos/p2").unwrap();
        store
            .insert(QuadRef::new(&photo2, &type_pred, &photo_type, &g))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo2,
                &NamedNode::new(&format!("{PICLOUD_NS}placeName")).unwrap(),
                &Literal::new_simple_literal("London"),
                &g,
            ))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo2,
                &NamedNode::new(&format!("{PICLOUD_NS}latitude")).unwrap(),
                &Literal::new_simple_literal("51.5074"),
                &g,
            ))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo2,
                &NamedNode::new(&format!("{PICLOUD_NS}longitude")).unwrap(),
                &Literal::new_simple_literal("-0.1278"),
                &g,
            ))
            .unwrap();
    }

    // ---- Step 2: Declare data product "photo-locations" in product A ----
    let dp_declared = make_data_product_declared(
        "photo-app",
        "photo-locations",
        "geospatial",
        "1.0.0",
    );
    projector.project(&dp_declared).await.unwrap();

    // Verify the data product exists in the RDF catalog as a pc:DataProduct
    let dp_resource_iri = ib
        .data_product_graph("photo-app", "photo-locations")
        .as_str()
        .trim_end_matches("/graph")
        .to_string();

    let ask_type = format!(
        "ASK {{ <{dp_resource_iri}> <{RDF_TYPE}> <{PICLOUD_NS}DataProduct> }}"
    );
    let result = projector.query(&ask_type).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "photo-locations data product should exist as a pc:DataProduct"
    );

    // Verify initial status is "declared"
    let ask_status = format!(
        "ASK {{ <{dp_resource_iri}> <{PICLOUD_NS}status> \"declared\" }}"
    );
    let result = projector.query(&ask_status).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product status should be 'declared' initially"
    );

    // Verify product and domain assignment
    let ask_product = format!(
        "ASK {{ <{dp_resource_iri}> <{PICLOUD_NS}product> \"photo-app\" }}"
    );
    let result = projector.query(&ask_product).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product should be scoped to photo-app"
    );

    let ask_domain = format!(
        "ASK {{ <{dp_resource_iri}> <{PICLOUD_NS}domain> \"geospatial\" }}"
    );
    let result = projector.query(&ask_domain).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product should belong to geospatial domain"
    );

    // ---- Step 3: Trigger projection (simulate PlaceResolved event) ----
    // The CONSTRUCT query projects photo locations from the product's
    // internal graph into the data product's published graph.
    let construct_query = format!(
        r#"CONSTRUCT {{
            ?photo <{PICLOUD_NS}placeName> ?place .
            ?photo <{PICLOUD_NS}latitude> ?lat .
            ?photo <{PICLOUD_NS}longitude> ?lon .
        }}
        WHERE {{
            GRAPH <{pg}> {{
                ?photo a <{PICLOUD_NS}Photo> ;
                       <{PICLOUD_NS}placeName> ?place ;
                       <{PICLOUD_NS}latitude> ?lat ;
                       <{PICLOUD_NS}longitude> ?lon .
            }}
        }}"#,
        pg = product_graph_iri.as_str(),
    );

    let dp_iri = ResourceIri::new(&dp_resource_iri).unwrap();
    let dp_projector =
        OxigraphDataProductProjector::new(Arc::new(projector.store().clone()));

    let refresh_result = dp_projector
        .refresh_projection(&dp_iri, &construct_query, &product_graph_iri)
        .await
        .unwrap();

    // 2 photos × 3 triples each = 6 triples
    assert_eq!(
        refresh_result.triple_count, 6,
        "CONSTRUCT should produce 6 triples (2 photos × 3 properties)"
    );

    // Record the DataProductRefreshed event in the RDF catalog
    let dp_refreshed = make_data_product_refreshed(
        "photo-app",
        "photo-locations",
        refresh_result.triple_count,
        "PlaceResolved",
    );
    projector.project(&dp_refreshed).await.unwrap();

    // ---- Step 4: Verify RDF catalog updated to "ready" ----
    // After DataProductRefreshed, update_status stores status as a named node
    // (picloud:Ready) and statusLabel as a literal ("ready").
    let ask_ready = format!(
        "ASK {{ <{dp_resource_iri}> <{PICLOUD_NS}statusLabel> \"ready\" }}"
    );
    let result = projector.query(&ask_ready).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product statusLabel should be 'ready' after refresh"
    );

    // Also verify the named node form
    let ask_ready_node = format!(
        "ASK {{ <{dp_resource_iri}> <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> }}"
    );
    let result = projector.query(&ask_ready_node).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product status should be picloud:Ready after refresh"
    );

    // Verify triple count is recorded
    let select_count = format!(
        "SELECT ?count WHERE {{ <{dp_resource_iri}> <{PICLOUD_NS}tripleCount> ?count }}"
    );
    let result = projector.query(&select_count).await.unwrap();
    assert_eq!(result.bindings.len(), 1, "tripleCount should be recorded");
    assert_eq!(
        result.bindings[0]["count"]["value"], "6",
        "tripleCount should be 6"
    );

    // ---- Step 5: Deploy product B ("maps-app") and query the data product ----
    let deploy_b = make_product_deployed("maps-app", "1.0.0");
    projector.project(&deploy_b).await.unwrap();

    // Product B queries the data product's published named graph
    let query_result = dp_projector
        .query_data_product(
            &dp_iri,
            &format!(
                "?photo <{PICLOUD_NS}placeName> ?place . \
                 ?photo <{PICLOUD_NS}latitude> ?lat"
            ),
        )
        .await
        .unwrap();

    assert_eq!(
        query_result.bindings.len(),
        2,
        "Product B should see 2 photo locations from the data product"
    );

    // Verify specific values are visible
    let places: Vec<String> = query_result
        .bindings
        .iter()
        .map(|b| b["place"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        places.contains(&"Paris".to_string()),
        "Paris should be visible in the data product"
    );
    assert!(
        places.contains(&"London".to_string()),
        "London should be visible in the data product"
    );

    // ---- Step 6: Update source data and re-trigger ----
    // Add a third photo (Tokyo) to the product A graph and remove Paris
    {
        use oxigraph::model::{Literal, NamedNode, QuadRef};

        let store = projector.store();
        let g = NamedNode::new(product_graph_iri.as_str()).unwrap();

        // Add Photo 3 — Tokyo
        let photo3 = NamedNode::new("https://picloud.local/products/photo-app/photos/p3").unwrap();
        let type_pred = NamedNode::new(RDF_TYPE).unwrap();
        let photo_type = NamedNode::new(&format!("{PICLOUD_NS}Photo")).unwrap();
        store
            .insert(QuadRef::new(&photo3, &type_pred, &photo_type, &g))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo3,
                &NamedNode::new(&format!("{PICLOUD_NS}placeName")).unwrap(),
                &Literal::new_simple_literal("Tokyo"),
                &g,
            ))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo3,
                &NamedNode::new(&format!("{PICLOUD_NS}latitude")).unwrap(),
                &Literal::new_simple_literal("35.6762"),
                &g,
            ))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo3,
                &NamedNode::new(&format!("{PICLOUD_NS}longitude")).unwrap(),
                &Literal::new_simple_literal("139.6503"),
                &g,
            ))
            .unwrap();

        // Remove Photo 1 (Paris) — simulates data change after trigger event
        let photo1 = NamedNode::new("https://picloud.local/products/photo-app/photos/p1").unwrap();
        // Remove all triples about photo1 from the product graph
        let quads: Vec<_> = store
            .quads_for_pattern(
                Some((&photo1).into()),
                None,
                None,
                Some(oxigraph::model::GraphNameRef::from(&g)),
            )
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        for quad in &quads {
            store.remove(quad).unwrap();
        }
    }

    // Re-trigger the projection (simulate a new PlaceResolved event)
    let refresh_result_2 = dp_projector
        .refresh_projection(&dp_iri, &construct_query, &product_graph_iri)
        .await
        .unwrap();

    // Now: London (p2) + Tokyo (p3) = 2 photos × 3 triples = 6 triples
    assert_eq!(
        refresh_result_2.triple_count, 6,
        "re-projection should produce 6 triples (2 remaining photos × 3 properties)"
    );

    // Record the second refresh
    let dp_refreshed_2 = make_data_product_refreshed(
        "photo-app",
        "photo-locations",
        refresh_result_2.triple_count,
        "PlaceResolved",
    );
    projector.project(&dp_refreshed_2).await.unwrap();

    // ---- Step 7: Product B re-queries and sees updated data ----
    let query_result_2 = dp_projector
        .query_data_product(
            &dp_iri,
            &format!(
                "?photo <{PICLOUD_NS}placeName> ?place . \
                 ?photo <{PICLOUD_NS}latitude> ?lat"
            ),
        )
        .await
        .unwrap();

    assert_eq!(
        query_result_2.bindings.len(),
        2,
        "Product B should now see 2 photos (Paris removed, Tokyo added)"
    );

    let updated_places: Vec<String> = query_result_2
        .bindings
        .iter()
        .map(|b| b["place"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        !updated_places.contains(&"Paris".to_string()),
        "Paris should be gone after re-projection (atomic swap removed stale data)"
    );
    assert!(
        updated_places.contains(&"London".to_string()),
        "London should still be visible"
    );
    assert!(
        updated_places.contains(&"Tokyo".to_string()),
        "Tokyo should be visible after re-projection"
    );

    // ---- Cross-cutting: Verify product isolation ----
    // The data product's named graph is separate from both products' operational graphs.
    let dp_graph_iri = ib.data_product_graph("photo-app", "photo-locations");
    assert!(
        dp_graph_iri.as_str().contains("/data-products/photo-locations/graph"),
        "data product graph IRI should follow the /data-products/ convention"
    );
    assert_ne!(
        dp_graph_iri.as_str(),
        product_graph_iri.as_str(),
        "data product graph must be separate from product A's operational graph"
    );

    // Verify the data product is linked to the owning product in the catalog
    let ask_produced_by = format!(
        "ASK {{ <{dp_resource_iri}> <{PICLOUD_NS}producedBy> <{}> }}",
        ib.product("photo-app").as_str()
    );
    let result = projector.query(&ask_produced_by).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "data product should be linked to photo-app via producedBy"
    );

    // Verify the data product graph is queryable through SPARQL
    // with full triple-pattern matching
    let full_query = dp_projector
        .query_data_product(
            &dp_iri,
            &format!(
                "?photo <{PICLOUD_NS}placeName> ?place ; \
                        <{PICLOUD_NS}latitude> ?lat ; \
                        <{PICLOUD_NS}longitude> ?lon"
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        full_query.bindings.len(),
        2,
        "full triple-pattern query should return 2 photo locations"
    );
}
