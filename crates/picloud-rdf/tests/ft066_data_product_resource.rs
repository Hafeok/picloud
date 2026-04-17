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

// ============================================================================
// TC-199 — data_product_named_graph_separation
// ============================================================================
/// After a projection run, query both the internal operational graph
/// (`…/products/photo-app/graph`) and the data product graph
/// (`…/data-products/photo-locations/graph`).
///
/// Assertions:
///   1. They are distinct named graphs (different IRIs, disjoint
///      triple sets when the CONSTRUCT is narrow enough).
///   2. The data product graph contains ONLY the triples produced by
///      the declared CONSTRUCT query. Any triple in the internal graph
///      that was not explicitly constructed must be absent from the
///      data product graph.
///   3. Mutating the internal graph does not implicitly mutate the
///      data product graph — the only way to refresh the data product
///      is to re-run the projection.
#[tokio::test]
async fn data_product_named_graph_separation() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    // ---- Deploy photo-app and seed its internal operational graph ----
    projector
        .project(&make_product_deployed("photo-app", "1.0.0"))
        .await
        .unwrap();

    let product_graph_iri = ib.product_graph("photo-app");
    {
        use oxigraph::model::{Literal, NamedNode, NamedNodeRef, QuadRef};
        let store = projector.store();
        store
            .insert_named_graph(NamedNodeRef::new(product_graph_iri.as_str()).unwrap())
            .unwrap();

        let g = NamedNode::new(product_graph_iri.as_str()).unwrap();
        let photo = NamedNode::new("https://picloud.local/products/photo-app/photos/p1").unwrap();
        let type_pred = NamedNode::new(RDF_TYPE).unwrap();
        let photo_type = NamedNode::new(&format!("{PICLOUD_NS}Photo")).unwrap();

        // Triple group A — exposed via CONSTRUCT: place + lat + lon.
        store.insert(QuadRef::new(&photo, &type_pred, &photo_type, &g)).unwrap();
        store
            .insert(QuadRef::new(
                &photo,
                &NamedNode::new(&format!("{PICLOUD_NS}placeName")).unwrap(),
                &Literal::new_simple_literal("Paris"),
                &g,
            ))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo,
                &NamedNode::new(&format!("{PICLOUD_NS}latitude")).unwrap(),
                &Literal::new_simple_literal("48.8566"),
                &g,
            ))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo,
                &NamedNode::new(&format!("{PICLOUD_NS}longitude")).unwrap(),
                &Literal::new_simple_literal("2.3522"),
                &g,
            ))
            .unwrap();

        // Triple group B — deliberately NOT projected by the CONSTRUCT.
        // These are operational/private and must not leak into the
        // published data product graph.
        store
            .insert(QuadRef::new(
                &photo,
                &NamedNode::new(&format!("{PICLOUD_NS}ownerEmail")).unwrap(),
                &Literal::new_simple_literal("secret@example.com"),
                &g,
            ))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo,
                &NamedNode::new(&format!("{PICLOUD_NS}rawExif")).unwrap(),
                &Literal::new_simple_literal("{camera:Canon,iso:400}"),
                &g,
            ))
            .unwrap();
    }

    // ---- Declare the data product and run the narrow CONSTRUCT projection ----
    projector
        .project(&make_data_product_declared(
            "photo-app",
            "photo-locations",
            "geospatial",
            "1.0.0",
        ))
        .await
        .unwrap();

    let dp_graph_iri = ib.data_product_graph("photo-app", "photo-locations");
    let dp_resource_iri = dp_graph_iri.as_str().trim_end_matches("/graph").to_string();
    let dp_iri = ResourceIri::new(&dp_resource_iri).unwrap();

    // CONSTRUCT projects only {placeName, latitude, longitude}.
    // `ownerEmail` and `rawExif` must stay in the operational graph.
    let construct_query = format!(
        r#"CONSTRUCT {{
            ?photo <{PICLOUD_NS}placeName> ?place .
            ?photo <{PICLOUD_NS}latitude> ?lat .
            ?photo <{PICLOUD_NS}longitude> ?lon .
        }} WHERE {{
            GRAPH <{pg}> {{
                ?photo a <{PICLOUD_NS}Photo> ;
                       <{PICLOUD_NS}placeName> ?place ;
                       <{PICLOUD_NS}latitude> ?lat ;
                       <{PICLOUD_NS}longitude> ?lon .
            }}
        }}"#,
        pg = product_graph_iri.as_str(),
    );

    let dp_projector = OxigraphDataProductProjector::new(Arc::new(projector.store().clone()));
    let refresh = dp_projector
        .refresh_projection(&dp_iri, &construct_query, &product_graph_iri)
        .await
        .unwrap();
    assert_eq!(refresh.triple_count, 3, "CONSTRUCT should emit 3 triples");

    // ---- Assertion 1: distinct graph IRIs ----
    assert_ne!(
        product_graph_iri.as_str(),
        dp_graph_iri.as_str(),
        "operational and data-product graphs must have distinct IRIs"
    );
    assert!(
        product_graph_iri.as_str().ends_with("/products/photo-app/graph"),
        "operational graph IRI must follow /products/<name>/graph: {}",
        product_graph_iri.as_str()
    );
    assert!(
        dp_graph_iri
            .as_str()
            .ends_with("/data-products/photo-locations/graph"),
        "data-product graph IRI must follow /data-products/<name>/graph: {}",
        dp_graph_iri.as_str()
    );

    // ---- Assertion 2: count triples in each graph ----
    // Operational graph: 1 type + 4 data triples = 5.
    let count_op = format!(
        "SELECT (COUNT(*) AS ?n) WHERE {{ GRAPH <{pg}> {{ ?s ?p ?o }} }}",
        pg = product_graph_iri.as_str()
    );
    let result = projector.query(&count_op).await.unwrap();
    let op_count: u64 = result.bindings[0]["n"]["value"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        op_count, 6,
        "operational graph should retain all 6 source triples (type + place + lat + lon + owner + exif)"
    );

    // Data product graph: exactly 3 triples (as reported by the CONSTRUCT).
    let count_dp = format!(
        "SELECT (COUNT(*) AS ?n) WHERE {{ GRAPH <{dp}> {{ ?s ?p ?o }} }}",
        dp = dp_graph_iri.as_str()
    );
    let result = projector.query(&count_dp).await.unwrap();
    let dp_count: u64 = result.bindings[0]["n"]["value"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        dp_count, 3,
        "data product graph should contain only CONSTRUCT output"
    );

    // ---- Assertion 3: internal-only triples do NOT appear in the dp graph ----
    let leak_owner = format!(
        "ASK {{ GRAPH <{dp}> {{ ?s <{PICLOUD_NS}ownerEmail> ?o }} }}",
        dp = dp_graph_iri.as_str()
    );
    let result = projector.query(&leak_owner).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "operational `ownerEmail` triple must not leak into the data product graph"
    );
    let leak_exif = format!(
        "ASK {{ GRAPH <{dp}> {{ ?s <{PICLOUD_NS}rawExif> ?o }} }}",
        dp = dp_graph_iri.as_str()
    );
    let result = projector.query(&leak_exif).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "operational `rawExif` triple must not leak into the data product graph"
    );
    // The `rdf:type <Photo>` triple is also internal — CONSTRUCT didn't emit it.
    let leak_type = format!(
        "ASK {{ GRAPH <{dp}> {{ ?s a <{PICLOUD_NS}Photo> }} }}",
        dp = dp_graph_iri.as_str()
    );
    let result = projector.query(&leak_type).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], false,
        "`rdf:type Photo` was not in the CONSTRUCT head — must not appear in dp graph"
    );

    // ---- Assertion 4: CONSTRUCT output IS present in the dp graph ----
    let has_place = format!(
        "ASK {{ GRAPH <{dp}> {{ ?s <{PICLOUD_NS}placeName> \"Paris\" }} }}",
        dp = dp_graph_iri.as_str()
    );
    let result = projector.query(&has_place).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "CONSTRUCT-projected `placeName` triple must be visible in the dp graph"
    );

    // ---- Assertion 5: the operational graph is untouched by projection ----
    // (Refreshing a data product must never mutate the source graph.)
    let op_still = format!(
        "ASK {{ GRAPH <{pg}> {{ ?s <{PICLOUD_NS}ownerEmail> \"secret@example.com\" }} }}",
        pg = product_graph_iri.as_str()
    );
    let result = projector.query(&op_still).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "operational graph must retain private triples after a dp projection run"
    );

    // ---- Assertion 6: graphs are queryable independently ----
    // A triple-pattern query scoped to one graph must not return results
    // that live in the other — definitive proof that named graph scoping
    // is honoured by the storage layer.
    let op_only = format!(
        "SELECT ?p ?o WHERE {{ GRAPH <{pg}> {{ <https://picloud.local/products/photo-app/photos/p1> ?p ?o }} }}",
        pg = product_graph_iri.as_str()
    );
    let op_result = projector.query(&op_only).await.unwrap();
    assert_eq!(
        op_result.bindings.len(),
        6,
        "operational graph should have 6 triples about photo p1"
    );

    let dp_only = format!(
        "SELECT ?p ?o WHERE {{ GRAPH <{dp}> {{ <https://picloud.local/products/photo-app/photos/p1> ?p ?o }} }}",
        dp = dp_graph_iri.as_str()
    );
    let dp_result = projector.query(&dp_only).await.unwrap();
    assert_eq!(
        dp_result.bindings.len(),
        3,
        "data product graph should have exactly 3 triples about photo p1"
    );
}

// ============================================================================
// TC-204 — data_product_deletion_guard
// ============================================================================
/// Attempt to delete `data-product 'photo-locations'` while `maps-app`
/// declares a `dataProducts` dependency on it.
///
/// Assertions:
///   1. `validate_data_product_deletion` is rejected with
///      `DataProductDeletionBlocked` and names the offending consumer count.
///   2. The data product remains fully intact in the catalog (it was not
///      half-deleted because the guard was never bypassed).
///   3. The data product's named graph is still queryable after the failed
///      delete — published triples survive.
///   4. Once the consumer dependency is removed, the guard passes and the
///      data product can be deleted cleanly (the guard is reversible,
///      not a permanent hold).
#[tokio::test]
async fn data_product_deletion_guard() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();
    let dp_projector = OxigraphDataProductProjector::new(Arc::new(projector.store().clone()));

    // ---- Deploy producer product + declare the data product ----
    projector
        .project(&make_product_deployed("photo-app", "1.0.0"))
        .await
        .unwrap();
    projector
        .project(&make_data_product_declared(
            "photo-app",
            "photo-locations",
            "geospatial",
            "1.0.0",
        ))
        .await
        .unwrap();

    // Seed the data product's named graph with a published triple so we
    // can later assert that the failed deletion left it intact.
    let product_graph_iri = ib.product_graph("photo-app");
    let dp_graph_iri = ib.data_product_graph("photo-app", "photo-locations");
    let dp_resource_iri = dp_graph_iri.as_str().trim_end_matches("/graph").to_string();
    let dp_iri = ResourceIri::new(&dp_resource_iri).unwrap();
    {
        use oxigraph::model::{Literal, NamedNode, NamedNodeRef, QuadRef};
        let store = projector.store();
        store
            .insert_named_graph(NamedNodeRef::new(product_graph_iri.as_str()).unwrap())
            .unwrap();
        let g = NamedNode::new(product_graph_iri.as_str()).unwrap();
        let photo = NamedNode::new("https://picloud.local/products/photo-app/photos/p1").unwrap();
        store
            .insert(QuadRef::new(
                &photo,
                &NamedNode::new(&format!("{PICLOUD_NS}placeName")).unwrap(),
                &Literal::new_simple_literal("Paris"),
                &g,
            ))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo,
                &NamedNode::new(&format!("{PICLOUD_NS}latitude")).unwrap(),
                &Literal::new_simple_literal("48.8566"),
                &g,
            ))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo,
                &NamedNode::new(&format!("{PICLOUD_NS}longitude")).unwrap(),
                &Literal::new_simple_literal("2.3522"),
                &g,
            ))
            .unwrap();
    }
    let construct = format!(
        r#"CONSTRUCT {{ ?s ?p ?o }} WHERE {{ GRAPH <{pg}> {{ ?s ?p ?o }} }}"#,
        pg = product_graph_iri.as_str(),
    );
    let refresh = dp_projector
        .refresh_projection(&dp_iri, &construct, &product_graph_iri)
        .await
        .unwrap();
    assert_eq!(refresh.triple_count, 3, "dp graph should be seeded with 3 triples");

    // ---- Deploy consumer product (`maps-app`) with a dataProducts dep ----
    // The ProductDeployed payload carries a `data_products` array so the
    // RDF projector can record the `pc:consumesDataProduct` link.
    let deploy_maps = {
        let ib = iri_builder();
        let product_iri = ib.product("maps-app");
        make_event(
            "ProductDeployed",
            product_iri.clone(),
            Some("maps-app"),
            serde_json::json!({
                "product_iri": product_iri.as_str(),
                "product_name": "maps-app",
                "version": "1.0.0",
                "data_products": [
                    { "source": "photo-app/photo-locations", "min_version": "1.0.0" }
                ],
            }),
        )
    };
    projector.project(&deploy_maps).await.unwrap();

    // ---- Sanity-check: the consumer link is in the catalog ----
    let ask_consumes = format!(
        "ASK {{ <{}> <{PICLOUD_NS}consumesDataProduct> <{dp_resource_iri}> }}",
        ib.product("maps-app").as_str(),
    );
    let result = projector.query(&ask_consumes).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "maps-app should declare a consumer link on photo-locations"
    );

    let consumers = projector
        .count_data_product_consumers(&dp_resource_iri)
        .unwrap();
    assert_eq!(
        consumers, 1,
        "exactly one consumer (maps-app) should be registered against photo-locations"
    );

    // ---- Assertion 1: delete is rejected with DataProductDeletionBlocked ----
    let result = projector.validate_data_product_deletion(&dp_resource_iri);
    assert!(
        result.is_err(),
        "delete must be rejected while maps-app declares the dependency"
    );
    match result.unwrap_err() {
        picloud_domain::error::PiCloudError::DataProductDeletionBlocked {
            data_product,
            consumers,
        } => {
            assert_eq!(
                data_product, dp_resource_iri,
                "blocked error must name the targeted data product"
            );
            assert_eq!(
                consumers, 1,
                "blocked error must report the consumer count (1)"
            );
        }
        other => panic!("expected DataProductDeletionBlocked, got: {other:?}"),
    }

    // ---- Assertion 2: data product resource triples are intact ----
    assert!(
        projector.data_product_exists(&dp_resource_iri).unwrap(),
        "data product must remain declared after a rejected delete"
    );
    let ask_domain = format!(
        "ASK {{ <{dp_resource_iri}> <{PICLOUD_NS}domain> \"geospatial\" }}"
    );
    let result = projector.query(&ask_domain).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "domain link must survive a rejected delete"
    );
    let ask_producer = format!(
        "ASK {{ <{dp_resource_iri}> <{PICLOUD_NS}producedBy> <{}> }}",
        ib.product("photo-app").as_str(),
    );
    let result = projector.query(&ask_producer).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "producer link must survive a rejected delete"
    );

    // ---- Assertion 3: the published named graph is still intact ----
    let count_dp = format!(
        "SELECT (COUNT(*) AS ?n) WHERE {{ GRAPH <{}> {{ ?s ?p ?o }} }}",
        dp_graph_iri.as_str()
    );
    let result = projector.query(&count_dp).await.unwrap();
    let n: u64 = result.bindings[0]["n"]["value"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        n, 3,
        "data product's named graph must retain its 3 triples after a rejected delete"
    );
    // And consumers can still query it via the projector API.
    let consumer_query = dp_projector
        .query_data_product(
            &dp_iri,
            &format!("?photo <{PICLOUD_NS}placeName> ?place"),
        )
        .await
        .unwrap();
    assert_eq!(
        consumer_query.bindings.len(),
        1,
        "maps-app must still be able to query photo-locations after a rejected delete"
    );

    // ---- Assertion 4: remove the consumer dependency → delete is allowed ----
    // Remove the `consumesDataProduct` triple (simulating `maps-app` being
    // re-deployed without the dependency, or being deleted entirely).
    {
        use oxigraph::model::{NamedNode, Quad};
        let subj = NamedNode::new(ib.product("maps-app").as_str()).unwrap();
        let pred = NamedNode::new(format!("{PICLOUD_NS}consumesDataProduct")).unwrap();
        let obj = NamedNode::new(&dp_resource_iri).unwrap();
        let store = projector.store();
        let _ = store.remove(&Quad::new(
            subj,
            pred,
            obj,
            oxigraph::model::GraphName::DefaultGraph,
        ));
    }
    let consumers_after = projector
        .count_data_product_consumers(&dp_resource_iri)
        .unwrap();
    assert_eq!(
        consumers_after, 0,
        "after removing the dependency, the consumer count must drop to zero"
    );
    projector
        .validate_data_product_deletion(&dp_resource_iri)
        .expect("deletion guard should pass once no consumers remain");
}
