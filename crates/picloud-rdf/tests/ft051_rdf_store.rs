/// FT-051 Integration Tests — Per-Product RDF Store (Managed Oxigraph)
///
/// Covers:
///   TC-264: Per-product Oxigraph instance created and serves SPARQL (scenario)
///   TC-321: RDF store exit — per-product Oxigraph created and serves SPARQL (exit-criteria)

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{RdfStoreManager, StateProjector};
use picloud_rdf::{OxigraphProjector, OxigraphRdfStoreManager};
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

fn make_rdf_store_declared(
    product: &str,
    name: &str,
    sparql_endpoint: &str,
    backing_volume: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let resource_iri = ib.resource(product, "rdf-store", name);
    make_event(
        "ResourceDeclared",
        Some(product),
        serde_json::json!({
            "resource_iri": resource_iri.as_str(),
            "resource_type": "RdfStore",
            "product": product,
            "name": name,
            "sparql_endpoint": sparql_endpoint,
            "backing_volume": backing_volume,
        }),
    )
}

fn make_resource_ready(product: &str, resource_type_slug: &str, name: &str) -> EventEnvelope {
    let ib = iri_builder();
    let resource_iri = ib.resource(product, resource_type_slug, name);
    make_event(
        "ResourceReady",
        Some(product),
        serde_json::json!({
            "resource_iri": resource_iri.as_str(),
        }),
    )
}

// ============================================================================
// TC-264 — Per-product Oxigraph instance created and serves SPARQL (scenario)
// ============================================================================
/// Scenario: a product declares an rdf-store resource, the platform creates a
/// per-product Oxigraph instance, and the product can issue SPARQL queries and
/// updates against that store in isolation from other products.
#[tokio::test]
async fn tc264_per_product_oxigraph_instance_created_and_serves_sparql() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();
    let manager = OxigraphRdfStoreManager::new();

    // --- Step 1: Deploy two products ---
    projector
        .project(&make_product_deployed("photo-app", "1.0.0"))
        .await
        .unwrap();
    projector
        .project(&make_product_deployed("chat-app", "1.0.0"))
        .await
        .unwrap();

    // --- Step 2: Declare an RdfStore resource for photo-app ---
    let photo_endpoint = ib.product_sparql("photo-app");
    let photo_volume = ib.resource("photo-app", "volume", "rdf-data");

    projector
        .project(&make_rdf_store_declared(
            "photo-app",
            "graph",
            photo_endpoint.as_str(),
            photo_volume.as_str(),
        ))
        .await
        .unwrap();

    // Make it Ready
    projector
        .project(&make_resource_ready("photo-app", "rdf-store", "graph"))
        .await
        .unwrap();

    // --- Step 3: Create the per-product Oxigraph instance ---
    manager.create_store("photo-app").await.unwrap();
    assert!(manager.has_store("photo-app").await.unwrap());
    assert!(!manager.has_store("chat-app").await.unwrap());

    // --- Step 4: Insert triples via SPARQL Update ---
    manager
        .update_store(
            "photo-app",
            r#"
            PREFIX ex: <https://photo-app.example.org/>
            PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
            INSERT DATA {
                ex:photo-001 rdf:type ex:Photo ;
                    ex:title "Sunset at the Beach" ;
                    ex:width "1920" ;
                    ex:height "1080" .
                ex:photo-002 rdf:type ex:Photo ;
                    ex:title "Mountain View" ;
                    ex:width "3840" ;
                    ex:height "2160" .
                ex:album-001 rdf:type ex:Album ;
                    ex:title "Vacation 2026" ;
                    ex:contains ex:photo-001 ;
                    ex:contains ex:photo-002 .
            }
            "#,
        )
        .await
        .unwrap();

    // --- Step 5: Query triples via SPARQL SELECT ---
    let result = manager
        .query_store(
            "photo-app",
            r#"
            PREFIX ex: <https://photo-app.example.org/>
            SELECT ?photo ?title WHERE {
                ?photo a ex:Photo ;
                    ex:title ?title .
            }
            ORDER BY ?title
            "#,
        )
        .await
        .unwrap();
    assert_eq!(
        result.bindings.len(),
        2,
        "Should find 2 photos, got: {:?}",
        result.bindings
    );
    // Check that titles are returned correctly
    let titles: Vec<&str> = result
        .bindings
        .iter()
        .filter_map(|b| b["title"]["value"].as_str())
        .collect();
    assert!(
        titles.contains(&"Mountain View"),
        "Should contain 'Mountain View'"
    );
    assert!(
        titles.contains(&"Sunset at the Beach"),
        "Should contain 'Sunset at the Beach'"
    );

    // --- Step 6: ASK query ---
    let result = manager
        .query_store(
            "photo-app",
            r#"
            PREFIX ex: <https://photo-app.example.org/>
            ASK { ex:album-001 ex:contains ex:photo-001 }
            "#,
        )
        .await
        .unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "Album should contain photo-001"
    );

    // --- Step 7: Verify isolation — chat-app has no store yet ---
    let err = manager.query_store("chat-app", "SELECT * WHERE { ?s ?p ?o }").await;
    assert!(
        err.is_err(),
        "Querying a non-existent store should error"
    );

    // Create chat-app store and verify it's empty (isolated from photo-app)
    manager.create_store("chat-app").await.unwrap();
    let result = manager
        .query_store("chat-app", "SELECT * WHERE { ?s ?p ?o }")
        .await
        .unwrap();
    assert_eq!(
        result.bindings.len(),
        0,
        "chat-app store should be empty — isolated from photo-app"
    );

    // --- Step 8: Verify the platform projector has the RdfStore resource ---
    let store_iri = ib.resource("photo-app", "rdf-store", "graph");
    let ask = format!(
        "ASK {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}RdfStore> }}",
        iri = store_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "RdfStore resource should be typed as picloud:RdfStore"
    );

    // Verify SPARQL endpoint triple
    let ask = format!(
        "ASK {{ <{iri}> <{PICLOUD_NS}sparqlEndpoint> <{endpoint}> }}",
        iri = store_iri.as_str(),
        endpoint = photo_endpoint.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "RdfStore should have sparqlEndpoint triple"
    );

    // Verify backing volume triple
    let ask = format!(
        "ASK {{ <{iri}> <{PICLOUD_NS}backingVolume> <{vol}> }}",
        iri = store_iri.as_str(),
        vol = photo_volume.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "RdfStore should have backingVolume triple"
    );

    // Verify the resource is Ready
    let ask = format!(
        "ASK {{ <{iri}> <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> }}",
        iri = store_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "RdfStore resource should be Ready"
    );

    // --- Step 9: Verify the RdfStore appears in the product's named graph ---
    let photo_graph = ib.product_graph("photo-app");
    let ask = format!(
        "ASK {{ GRAPH <{graph}> {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}RdfStore> }} }}",
        graph = photo_graph.as_str(),
        iri = store_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "RdfStore should appear in photo-app's named graph"
    );

    // --- Step 10: Drop store and verify ---
    manager.drop_store("photo-app").await.unwrap();
    assert!(!manager.has_store("photo-app").await.unwrap());
}

// ============================================================================
// TC-321 — RDF store exit — per-product Oxigraph created and serves SPARQL
// ============================================================================
/// Exit criteria: end-to-end verification that a product can declare an
/// rdf-store resource, the platform projects it into the RDF graph with the
/// correct type and SPARQL endpoint IRI, a per-product Oxigraph instance
/// is created, and SPARQL query + update operations succeed against the
/// isolated store.
#[tokio::test]
async fn tc321_rdf_store_exit_per_product_oxigraph_created_and_serves_sparql() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();
    let manager = OxigraphRdfStoreManager::new();

    // --- Deploy product and declare rdf-store ---
    let product_name = "analytics-app";
    projector
        .project(&make_product_deployed(product_name, "3.0.0"))
        .await
        .unwrap();

    let sparql_endpoint = ib.product_sparql(product_name);
    let backing_volume = ib.resource(product_name, "volume", "rdf-storage");

    projector
        .project(&make_rdf_store_declared(
            product_name,
            "graph",
            sparql_endpoint.as_str(),
            backing_volume.as_str(),
        ))
        .await
        .unwrap();
    projector
        .project(&make_resource_ready(product_name, "rdf-store", "graph"))
        .await
        .unwrap();

    // --- Verify platform projection ---
    let store_iri = ib.resource(product_name, "rdf-store", "graph");

    // 1. rdf:type picloud:RdfStore
    let ask = format!(
        "ASK {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}RdfStore> }}",
        iri = store_iri.as_str()
    );
    let r = projector.query(&ask).await.unwrap();
    assert_eq!(r.bindings[0]["result"], true, "should be typed as RdfStore");

    // 2. picloud:sparqlEndpoint points to the correct IRI
    let q = format!(
        "SELECT ?ep WHERE {{ <{iri}> <{PICLOUD_NS}sparqlEndpoint> ?ep }}",
        iri = store_iri.as_str()
    );
    let r = projector.query(&q).await.unwrap();
    assert_eq!(r.bindings.len(), 1, "should have exactly one sparqlEndpoint");
    assert_eq!(
        r.bindings[0]["ep"]["value"].as_str().unwrap(),
        sparql_endpoint.as_str(),
        "sparqlEndpoint IRI should match"
    );

    // 3. picloud:backingVolume points to the correct IRI
    let q = format!(
        "SELECT ?vol WHERE {{ <{iri}> <{PICLOUD_NS}backingVolume> ?vol }}",
        iri = store_iri.as_str()
    );
    let r = projector.query(&q).await.unwrap();
    assert_eq!(r.bindings.len(), 1, "should have exactly one backingVolume");
    assert_eq!(
        r.bindings[0]["vol"]["value"].as_str().unwrap(),
        backing_volume.as_str(),
        "backingVolume IRI should match"
    );

    // 4. Status is Ready
    let ask = format!(
        "ASK {{ <{iri}> <{PICLOUD_NS}status> <{PICLOUD_NS}Ready> }}",
        iri = store_iri.as_str()
    );
    let r = projector.query(&ask).await.unwrap();
    assert_eq!(r.bindings[0]["result"], true, "status should be Ready");

    // 5. Appears in product's named graph as RdfStore
    let graph = ib.product_graph(product_name);
    let ask = format!(
        "ASK {{ GRAPH <{graph}> {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}RdfStore> }} }}",
        graph = graph.as_str(),
        iri = store_iri.as_str()
    );
    let r = projector.query(&ask).await.unwrap();
    assert_eq!(r.bindings[0]["result"], true, "should be in named graph");

    // --- Create the per-product Oxigraph instance ---
    manager.create_store(product_name).await.unwrap();
    assert!(manager.has_store(product_name).await.unwrap());

    // --- SPARQL Update: insert domain triples ---
    manager
        .update_store(
            product_name,
            r#"
            PREFIX schema: <https://schema.org/>
            PREFIX analytics: <https://analytics-app.example.org/>
            INSERT DATA {
                analytics:event-001 a schema:Event ;
                    schema:name "PageView" ;
                    schema:location "https://example.com/home" .
                analytics:event-002 a schema:Event ;
                    schema:name "Click" ;
                    schema:location "https://example.com/pricing" .
                analytics:event-003 a schema:Event ;
                    schema:name "PageView" ;
                    schema:location "https://example.com/pricing" .
            }
            "#,
        )
        .await
        .unwrap();

    // --- SPARQL SELECT: count events ---
    let result = manager
        .query_store(
            product_name,
            r#"
            PREFIX schema: <https://schema.org/>
            SELECT (COUNT(?e) AS ?count) WHERE {
                ?e a schema:Event .
            }
            "#,
        )
        .await
        .unwrap();
    let count: i64 = result.bindings[0]["count"]["value"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(count, 3, "should have 3 events");

    // --- SPARQL SELECT: filter by name ---
    let result = manager
        .query_store(
            product_name,
            r#"
            PREFIX schema: <https://schema.org/>
            SELECT ?e WHERE {
                ?e a schema:Event ;
                    schema:name "PageView" .
            }
            "#,
        )
        .await
        .unwrap();
    assert_eq!(
        result.bindings.len(),
        2,
        "should find 2 PageView events"
    );

    // --- SPARQL CONSTRUCT ---
    let result = manager
        .query_store(
            product_name,
            r#"
            PREFIX schema: <https://schema.org/>
            CONSTRUCT {
                ?e schema:name ?name .
            } WHERE {
                ?e a schema:Event ;
                    schema:name ?name .
            }
            "#,
        )
        .await
        .unwrap();
    assert_eq!(
        result.bindings.len(),
        3,
        "CONSTRUCT should return 3 triples"
    );

    // --- SPARQL ASK ---
    let result = manager
        .query_store(
            product_name,
            r#"
            PREFIX schema: <https://schema.org/>
            ASK {
                ?e a schema:Event ;
                    schema:name "Click" .
            }
            "#,
        )
        .await
        .unwrap();
    assert_eq!(result.bindings[0]["result"], true, "Click event should exist");

    // --- SPARQL DELETE ---
    manager
        .update_store(
            product_name,
            r#"
            PREFIX schema: <https://schema.org/>
            PREFIX analytics: <https://analytics-app.example.org/>
            DELETE DATA {
                analytics:event-002 a schema:Event ;
                    schema:name "Click" ;
                    schema:location "https://example.com/pricing" .
            }
            "#,
        )
        .await
        .unwrap();

    let result = manager
        .query_store(
            product_name,
            r#"
            PREFIX schema: <https://schema.org/>
            SELECT (COUNT(?e) AS ?count) WHERE {
                ?e a schema:Event .
            }
            "#,
        )
        .await
        .unwrap();
    let count: i64 = result.bindings[0]["count"]["value"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(count, 2, "should have 2 events after deletion");

    // --- Verify product isolation: second product sees nothing ---
    let other = "other-app";
    manager.create_store(other).await.unwrap();
    let result = manager
        .query_store(other, "SELECT (COUNT(*) AS ?c) WHERE { ?s ?p ?o }")
        .await
        .unwrap();
    let c: i64 = result.bindings[0]["c"]["value"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(c, 0, "other-app store should be empty — isolated from analytics-app");

    // --- Cleanup ---
    manager.drop_store(product_name).await.unwrap();
    manager.drop_store(other).await.unwrap();
    assert!(!manager.has_store(product_name).await.unwrap());
    assert!(!manager.has_store(other).await.unwrap());
}
