/// FT-054 Integration Tests — RDFS/OWL inference enabled on platform and product graphs
///
/// Covers:
///   TC-267: RDFS/OWL inference derives transitive triples in product graph
///   TC-324: Inference exit — RDFS/OWL transitive triples derived

use oxigraph::model::NamedNode;
use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::StateProjector;
use picloud_rdf::OxigraphProjector;
use uuid::Uuid;

const PICLOUD_NS: &str = "https://picloud.local/ontology#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDFS_SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";

fn picloud_term(local: &str) -> oxigraph::model::Term {
    NamedNode::new(format!("{PICLOUD_NS}{local}"))
        .expect("valid picloud ontology IRI")
        .into()
}

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

fn make_ontology_declared(
    product: &str,
    name: &str,
    file_path: &str,
    format: &str,
    version: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let resource_iri = ib.resource(product, "ontology", name);
    let served_at = ib.product_ontology_versioned(product, version);
    make_event(
        "ResourceDeclared",
        Some(product),
        serde_json::json!({
            "resource_iri": resource_iri.as_str(),
            "resource_type": "Ontology",
            "product": product,
            "name": name,
            "file_path": file_path,
            "format": format,
            "served_at": served_at.as_str(),
            "version": version,
        }),
    )
}

fn make_ontology_loaded(
    product: &str,
    ontology_name: &str,
    version: &str,
    content: &str,
    format: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let ontology_iri = ib.resource(product, "ontology", ontology_name);
    make_event(
        "OntologyLoaded",
        Some(product),
        serde_json::json!({
            "ontology_iri": ontology_iri.as_str(),
            "product": product,
            "version": version,
            "content": content,
            "format": format,
        }),
    )
}

// ============================================================================
// TC-267 — RDFS/OWL inference derives transitive triples in product graph
// ============================================================================
/// Scenario test: Deploy a product with an ontology containing both RDFS
/// subClassOf hierarchies and OWL TransitiveProperty declarations.
/// Verify that:
///  1. RDFS subclass inference derives transitive type triples
///  2. OWL transitive-property inference derives transitive relationship triples
///  3. Inferred triples exist in the product's named graph
///  4. Transitive chains of depth >= 3 are fully materialised
#[tokio::test]
async fn tc267_rdfs_owl_inference_derives_transitive_triples_in_product_graph() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    let product = "infra-app";
    let version = "1.0.0";

    // --- Step 1: Deploy the product ---
    projector
        .project(&make_product_deployed(product, version))
        .await
        .unwrap();

    // --- Step 2: Declare and load a Turtle ontology with both RDFS and OWL axioms ---
    let ttl_decl = make_ontology_declared(
        product,
        "infra-ontology",
        "ontology/infra.ttl",
        "turtle",
        version,
    );
    projector.project(&ttl_decl).await.unwrap();

    let turtle_content = r#"
        @prefix picloud: <https://picloud.local/ontology#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
        @prefix owl: <http://www.w3.org/2002/07/owl#> .

        # RDFS subclass hierarchy (depth 3): StagingContainer < ProductionContainer < Container
        picloud:Container a rdfs:Class .
        picloud:ProductionContainer rdfs:subClassOf picloud:Container .
        picloud:StagingContainer rdfs:subClassOf picloud:ProductionContainer .

        # OWL transitive property: dependsOn
        picloud:dependsOn a owl:TransitiveProperty .
    "#;
    let load_ttl = make_ontology_loaded(
        product,
        "infra-ontology",
        version,
        turtle_content,
        "turtle",
    );
    projector.project(&load_ttl).await.unwrap();

    // --- Step 3: Insert instances with RDFS subclass types ---
    // Insert an instance of StagingContainer (the deepest subclass)
    let staging_iri = "https://picloud.local/products/infra-app/containers/staging-api";
    projector
        .insert_triple(staging_iri, RDF_TYPE, picloud_term("StagingContainer").into())
        .unwrap();
    projector.materialise_rdfs_subclass().unwrap();

    // Verify transitive RDFS subclass inference:
    // StagingContainer → ProductionContainer → Container
    let ask = format!(
        "ASK {{ <{staging_iri}> <{RDF_TYPE}> <{PICLOUD_NS}ProductionContainer> }}"
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "StagingContainer instance should be inferred as ProductionContainer (direct parent)"
    );

    let ask = format!(
        "ASK {{ <{staging_iri}> <{RDF_TYPE}> <{PICLOUD_NS}Container> }}"
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "StagingContainer instance should be inferred as Container (transitive grandparent)"
    );

    // --- Step 4: Insert OWL transitive-property assertions ---
    // Chain: photo-app dependsOn user-service dependsOn auth-service dependsOn cert-service
    let photo_iri = "https://picloud.local/products/infra-app/services/photo-app";
    let user_iri = "https://picloud.local/products/infra-app/services/user-service";
    let auth_iri = "https://picloud.local/products/infra-app/services/auth-service";
    let cert_iri = "https://picloud.local/products/infra-app/services/cert-service";

    let depends_on = format!("{PICLOUD_NS}dependsOn");
    projector
        .insert_triple(
            photo_iri,
            &depends_on,
            NamedNode::new(user_iri).unwrap().into(),
        )
        .unwrap();
    projector
        .insert_triple(
            user_iri,
            &depends_on,
            NamedNode::new(auth_iri).unwrap().into(),
        )
        .unwrap();
    projector
        .insert_triple(
            auth_iri,
            &depends_on,
            NamedNode::new(cert_iri).unwrap().into(),
        )
        .unwrap();

    // Run transitive-property materialisation
    let inferred = projector.materialise_owl_transitive_property().unwrap();
    assert!(
        inferred > 0,
        "Should have inferred at least one transitive triple, got {inferred}"
    );

    // Verify depth-2 inference: photo-app dependsOn auth-service
    let ask = format!("ASK {{ <{photo_iri}> <{depends_on}> <{auth_iri}> }}");
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "photo-app should transitively dependOn auth-service (depth-2)"
    );

    // Verify depth-3 inference: photo-app dependsOn cert-service
    let ask = format!("ASK {{ <{photo_iri}> <{depends_on}> <{cert_iri}> }}");
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "photo-app should transitively dependOn cert-service (depth-3)"
    );

    // Verify depth-2 inference: user-service dependsOn cert-service
    let ask = format!("ASK {{ <{user_iri}> <{depends_on}> <{cert_iri}> }}");
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "user-service should transitively dependOn cert-service (depth-2)"
    );

    // --- Step 5: Verify transitive triples are in the product's named graph ---
    // Insert the same chain into the product graph and verify inference there
    let graph_iri = ib.product_graph(product);
    let svc_a = "https://picloud.local/products/infra-app/services/svc-a";
    let svc_b = "https://picloud.local/products/infra-app/services/svc-b";
    let svc_c = "https://picloud.local/products/infra-app/services/svc-c";

    projector
        .insert_triple_in_graph(
            svc_a,
            &depends_on,
            NamedNode::new(svc_b).unwrap().into(),
            graph_iri.as_str(),
        )
        .unwrap();
    projector
        .insert_triple_in_graph(
            svc_b,
            &depends_on,
            NamedNode::new(svc_c).unwrap().into(),
            graph_iri.as_str(),
        )
        .unwrap();

    let inferred = projector.materialise_owl_transitive_property().unwrap();
    assert!(
        inferred > 0,
        "Should infer transitive triples in product named graph"
    );

    // Verify: svc-a dependsOn svc-c in product graph
    let ask = format!(
        "ASK {{ GRAPH <{graph}> {{ <{svc_a}> <{depends_on}> <{svc_c}> }} }}",
        graph = graph_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "Transitive inference should work within product named graph"
    );

    // --- Step 6: Verify SPARQL query can discover full dependency chain ---
    let q = format!(
        "SELECT ?dep WHERE {{ <{photo_iri}> <{depends_on}> ?dep }}"
    );
    let result = projector.query(&q).await.unwrap();
    // Should have direct + inferred: user-service, auth-service, cert-service
    assert!(
        result.bindings.len() >= 3,
        "photo-app should depend on at least 3 services (direct + inferred), got {}",
        result.bindings.len()
    );
}

// ============================================================================
// TC-324 — Inference exit — RDFS/OWL transitive triples derived
// ============================================================================
/// Exit criteria: End-to-end validation of RDFS/OWL inference across the
/// platform and product graphs:
///  1. RDFS subclass inference active after ontology deployment
///  2. OWL transitive-property closure inferred for depth-3 chains
///  3. Inference materialised during ontology load (not requiring manual call)
///  4. Inferred triples queryable in both default and product named graphs
///  5. SPARQL queries automatically include inferred triples
///  6. Multiple transitive properties coexist correctly
///  7. Mixed RDFS + OWL inference in same ontology
#[tokio::test]
async fn tc324_inference_exit_rdfs_owl_transitive_triples_derived() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    let product = "logistics-app";
    let version = "3.0.0";

    // === Phase 1: Deploy product ===
    projector
        .project(&make_product_deployed(product, version))
        .await
        .unwrap();

    // === Phase 2: Load ontology with RDFS + OWL axioms ===
    let ttl_decl = make_ontology_declared(
        product,
        "logistics-model",
        "ontology/logistics.ttl",
        "turtle",
        version,
    );
    projector.project(&ttl_decl).await.unwrap();

    // Ontology declares both RDFS subclass hierarchy and multiple OWL transitive properties
    let turtle_content = r#"
        @prefix picloud: <https://picloud.local/ontology#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
        @prefix owl: <http://www.w3.org/2002/07/owl#> .

        # RDFS class hierarchy (depth 3)
        picloud:Vehicle a rdfs:Class .
        picloud:Truck rdfs:subClassOf picloud:Vehicle .
        picloud:HeavyTruck rdfs:subClassOf picloud:Truck .

        # OWL transitive property #1: routeConnects
        picloud:routeConnects a owl:TransitiveProperty .

        # OWL transitive property #2: containedIn
        picloud:containedIn a owl:TransitiveProperty .
    "#;
    let load_ttl = make_ontology_loaded(
        product,
        "logistics-model",
        version,
        turtle_content,
        "turtle",
    );
    projector.project(&load_ttl).await.unwrap();

    // === Phase 3: Verify RDFS inference is active after ontology load ===
    // Insert HeavyTruck instance — should auto-infer Truck and Vehicle types
    // after explicit materialise call (inference runs on load for ontology triples,
    // but new instance assertions require materialise)
    let truck_iri = "https://picloud.local/products/logistics-app/fleet/ht-001";
    projector
        .insert_triple(truck_iri, RDF_TYPE, picloud_term("HeavyTruck").into())
        .unwrap();
    projector.materialise_rdfs_subclass().unwrap();

    // HeavyTruck → Truck (direct)
    let ask = format!("ASK {{ <{truck_iri}> <{RDF_TYPE}> <{PICLOUD_NS}Truck> }}");
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "HeavyTruck instance should be inferred as Truck"
    );

    // HeavyTruck → Vehicle (transitive)
    let ask = format!("ASK {{ <{truck_iri}> <{RDF_TYPE}> <{PICLOUD_NS}Vehicle> }}");
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "HeavyTruck instance should be inferred as Vehicle (transitive)"
    );

    // === Phase 4: OWL transitive-property #1 — routeConnects (depth-3) ===
    let route_pred = format!("{PICLOUD_NS}routeConnects");
    let depot_a = "https://picloud.local/products/logistics-app/depots/a";
    let depot_b = "https://picloud.local/products/logistics-app/depots/b";
    let depot_c = "https://picloud.local/products/logistics-app/depots/c";
    let depot_d = "https://picloud.local/products/logistics-app/depots/d";

    projector
        .insert_triple(depot_a, &route_pred, NamedNode::new(depot_b).unwrap().into())
        .unwrap();
    projector
        .insert_triple(depot_b, &route_pred, NamedNode::new(depot_c).unwrap().into())
        .unwrap();
    projector
        .insert_triple(depot_c, &route_pred, NamedNode::new(depot_d).unwrap().into())
        .unwrap();

    let inferred = projector.materialise_owl_transitive_property().unwrap();
    assert!(
        inferred >= 3,
        "Should infer at least 3 transitive triples from depth-3 chain: a→c, b→d, a→d; got {inferred}"
    );

    // Verify depth-2: a routeConnects c
    let ask = format!("ASK {{ <{depot_a}> <{route_pred}> <{depot_c}> }}");
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "depot-a should routeConnect depot-c (depth-2)"
    );

    // Verify depth-3: a routeConnects d
    let ask = format!("ASK {{ <{depot_a}> <{route_pred}> <{depot_d}> }}");
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "depot-a should routeConnect depot-d (depth-3)"
    );

    // Verify depth-2: b routeConnects d
    let ask = format!("ASK {{ <{depot_b}> <{route_pred}> <{depot_d}> }}");
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "depot-b should routeConnect depot-d (depth-2)"
    );

    // === Phase 5: OWL transitive-property #2 — containedIn (coexists with routeConnects) ===
    let contained_pred = format!("{PICLOUD_NS}containedIn");
    let shelf = "https://picloud.local/products/logistics-app/locations/shelf-1";
    let room = "https://picloud.local/products/logistics-app/locations/room-a";
    let building = "https://picloud.local/products/logistics-app/locations/building-x";

    projector
        .insert_triple(shelf, &contained_pred, NamedNode::new(room).unwrap().into())
        .unwrap();
    projector
        .insert_triple(room, &contained_pred, NamedNode::new(building).unwrap().into())
        .unwrap();

    let inferred = projector.materialise_owl_transitive_property().unwrap();
    assert!(
        inferred >= 1,
        "Should infer containedIn transitive triple; got {inferred}"
    );

    // shelf containedIn building (transitive)
    let ask = format!("ASK {{ <{shelf}> <{contained_pred}> <{building}> }}");
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "shelf should be containedIn building (transitive)"
    );

    // === Phase 6: Inference during ontology load (automatic materialisation) ===
    // Load a second ontology that contains pre-asserted transitive chains
    let second_turtle = r#"
        @prefix picloud: <https://picloud.local/ontology#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
        @prefix owl: <http://www.w3.org/2002/07/owl#> .

        # Transitive property with instances declared in the same ontology
        picloud:partOf a owl:TransitiveProperty .
        picloud:wheel1 picloud:partOf picloud:axle1 .
        picloud:axle1 picloud:partOf picloud:chassis1 .
        picloud:chassis1 picloud:partOf picloud:truck1 .
    "#;

    let second_decl = make_ontology_declared(
        product,
        "parts-model",
        "ontology/parts.ttl",
        "turtle",
        version,
    );
    projector.project(&second_decl).await.unwrap();

    let load_second = make_ontology_loaded(
        product,
        "parts-model",
        version,
        second_turtle,
        "turtle",
    );
    projector.project(&load_second).await.unwrap();

    // Inferred triples should already exist (materialised during ontology load)
    // wheel1 partOf chassis1 (depth-2)
    let part_of_pred = format!("{PICLOUD_NS}partOf");
    let ask = format!(
        "ASK {{ <{PICLOUD_NS}wheel1> <{part_of_pred}> <{PICLOUD_NS}chassis1> }}"
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "wheel1 should be partOf chassis1 via automatic materialisation during load"
    );

    // wheel1 partOf truck1 (depth-3)
    let ask = format!(
        "ASK {{ <{PICLOUD_NS}wheel1> <{part_of_pred}> <{PICLOUD_NS}truck1> }}"
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "wheel1 should be partOf truck1 via automatic materialisation during load (depth-3)"
    );

    // axle1 partOf truck1 (depth-2)
    let ask = format!(
        "ASK {{ <{PICLOUD_NS}axle1> <{part_of_pred}> <{PICLOUD_NS}truck1> }}"
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "axle1 should be partOf truck1 via automatic materialisation"
    );

    // === Phase 7: Verify in product named graph ===
    let graph_iri = ib.product_graph(product);

    // Ontology triples loaded into product graph should also have inference
    let ask = format!(
        "ASK {{ GRAPH <{graph}> {{ <{PICLOUD_NS}wheel1> <{part_of_pred}> <{PICLOUD_NS}truck1> }} }}",
        graph = graph_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "Transitive inference should be in product named graph"
    );

    // === Phase 8: SPARQL query automatically includes inferred triples ===
    // Count all things wheel1 is partOf — should include axle1, chassis1, truck1
    let q = format!(
        "SELECT ?parent WHERE {{ <{PICLOUD_NS}wheel1> <{part_of_pred}> ?parent }}"
    );
    let result = projector.query(&q).await.unwrap();
    assert!(
        result.bindings.len() >= 3,
        "wheel1 should be partOf at least 3 things (direct + inferred), got {}",
        result.bindings.len()
    );

    // === Phase 9: Mixed RDFS + OWL — both inference types work from same ontology ===
    // The RDFS subclass hierarchy (Vehicle/Truck/HeavyTruck) was loaded in same
    // ontology as OWL transitive properties. Verify both are still consistent.
    let ask = format!(
        "ASK {{ <{PICLOUD_NS}HeavyTruck> <{RDFS_SUBCLASS_OF}> <{PICLOUD_NS}Truck> }}"
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "RDFS subclass should still be present alongside OWL inference"
    );

    // Verify routeConnects inference is still correct
    let ask = format!("ASK {{ <{depot_a}> <{route_pred}> <{depot_d}> }}");
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "OWL transitive inference should persist after additional ontology loads"
    );

    // === Phase 10: Count total transitive properties in the system ===
    let q = format!(
        "SELECT (COUNT(DISTINCT ?p) AS ?count) WHERE {{ ?p <{RDF_TYPE}> <http://www.w3.org/2002/07/owl#TransitiveProperty> }}"
    );
    let result = projector.query(&q).await.unwrap();
    let count: usize = result.bindings[0]["count"]["value"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        count >= 3,
        "Should have at least 3 transitive properties (routeConnects, containedIn, partOf), got {count}"
    );
}
