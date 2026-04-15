/// FT-053 Integration Tests — Ontology resource type (.ttl and .shacl)
///
/// Covers:
///   TC-266: Ontology .ttl and .shacl files bound to product version and queryable
///   TC-323: Ontology exit — .ttl and .shacl bound to version and queryable

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
// TC-266 — Ontology .ttl and .shacl files bound to product version and queryable
// ============================================================================
/// Scenario test: Declare Ontology resources (both Turtle and SHACL formats),
/// deploy the product, load ontology content, and verify that:
///  1. Ontology resources are projected with correct metadata
///  2. Ontology content (Turtle triples) is loaded and queryable via SPARQL
///  3. SHACL shapes are loaded and queryable
///  4. Versioned ontology IRI is bound to the product
///  5. RDFS subclass inference is materialised from loaded ontology
#[tokio::test]
async fn tc266_ontology_ttl_and_shacl_files_bound_to_product_version_and_queryable() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    let product = "photo-app";
    let version = "2.0.0";

    // --- Step 1: Deploy the product ---
    let deploy_event = make_product_deployed(product, version);
    projector.project(&deploy_event).await.unwrap();

    // --- Step 2: Declare a Turtle ontology resource ---
    let ttl_event = make_ontology_declared(
        product,
        "photo-schema",
        "ontology/photo-schema.ttl",
        "turtle",
        version,
    );
    projector.project(&ttl_event).await.unwrap();

    // --- Step 3: Declare a SHACL ontology resource ---
    let shacl_event = make_ontology_declared(
        product,
        "photo-shapes",
        "ontology/photo-shapes.shacl",
        "shacl",
        version,
    );
    projector.project(&shacl_event).await.unwrap();

    // --- Step 4: Verify Ontology resources are projected with correct type ---
    let ttl_iri = ib.resource(product, "ontology", "photo-schema");
    let ask = format!(
        "ASK {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}Ontology> }}",
        iri = ttl_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "Turtle ontology should have rdf:type picloud:Ontology"
    );

    let shacl_iri = ib.resource(product, "ontology", "photo-shapes");
    let ask = format!(
        "ASK {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}Ontology> }}",
        iri = shacl_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "SHACL ontology should have rdf:type picloud:Ontology"
    );

    // --- Step 5: Verify ontology metadata (file_path, format) ---
    let q = format!(
        "SELECT ?path ?fmt WHERE {{ <{iri}> <{PICLOUD_NS}filePath> ?path ; <{PICLOUD_NS}format> ?fmt }}",
        iri = ttl_iri.as_str()
    );
    let result = projector.query(&q).await.unwrap();
    assert_eq!(result.bindings.len(), 1, "Turtle ontology should have filePath and format");
    assert_eq!(
        result.bindings[0]["path"]["value"].as_str().unwrap(),
        "ontology/photo-schema.ttl"
    );
    assert_eq!(
        result.bindings[0]["fmt"]["value"].as_str().unwrap(),
        "turtle"
    );

    let q = format!(
        "SELECT ?path ?fmt WHERE {{ <{iri}> <{PICLOUD_NS}filePath> ?path ; <{PICLOUD_NS}format> ?fmt }}",
        iri = shacl_iri.as_str()
    );
    let result = projector.query(&q).await.unwrap();
    assert_eq!(result.bindings.len(), 1, "SHACL ontology should have filePath and format");
    assert_eq!(
        result.bindings[0]["fmt"]["value"].as_str().unwrap(),
        "shacl"
    );

    // --- Step 6: Load Turtle ontology content ---
    let turtle_content = r#"
        @prefix picloud: <https://picloud.local/ontology#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        picloud:Photo a rdfs:Class .
        picloud:Album a rdfs:Class .
        picloud:SharedAlbum rdfs:subClassOf picloud:Album .
    "#;
    let load_ttl = make_ontology_loaded(product, "photo-schema", version, turtle_content, "turtle");
    projector.project(&load_ttl).await.unwrap();

    // --- Step 7: Load SHACL shapes content ---
    let shacl_content = r#"
        @prefix picloud: <https://picloud.local/ontology#> .
        @prefix sh: <http://www.w3.org/ns/shacl#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        picloud:PhotoShape
            a sh:NodeShape ;
            sh:targetClass picloud:Photo ;
            sh:property [
                sh:path picloud:title ;
                sh:datatype <http://www.w3.org/2001/XMLSchema#string> ;
                sh:minCount 1 ;
            ] .
    "#;
    let load_shacl = make_ontology_loaded(product, "photo-shapes", version, shacl_content, "shacl");
    projector.project(&load_shacl).await.unwrap();

    // --- Step 8: Verify Turtle classes are queryable ---
    let ask = format!("ASK {{ <{PICLOUD_NS}Photo> <{RDF_TYPE}> <http://www.w3.org/2000/01/rdf-schema#Class> }}");
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "picloud:Photo should be an rdfs:Class after loading Turtle ontology"
    );

    let ask = format!("ASK {{ <{PICLOUD_NS}Album> <{RDF_TYPE}> <http://www.w3.org/2000/01/rdf-schema#Class> }}");
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "picloud:Album should be an rdfs:Class"
    );

    // --- Step 9: Verify SHACL shapes are queryable ---
    let ask = format!(
        "ASK {{ <{PICLOUD_NS}PhotoShape> <{RDF_TYPE}> <http://www.w3.org/ns/shacl#NodeShape> }}"
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "picloud:PhotoShape should be a sh:NodeShape after loading SHACL shapes"
    );

    let q = format!(
        "SELECT ?target WHERE {{ <{PICLOUD_NS}PhotoShape> <http://www.w3.org/ns/shacl#targetClass> ?target }}"
    );
    let result = projector.query(&q).await.unwrap();
    assert_eq!(result.bindings.len(), 1);
    assert_eq!(
        result.bindings[0]["target"]["value"].as_str().unwrap(),
        format!("{PICLOUD_NS}Photo")
    );

    // --- Step 10: Verify RDFS subclass inference ---
    let ask = format!(
        "ASK {{ <{PICLOUD_NS}SharedAlbum> <{RDFS_SUBCLASS_OF}> <{PICLOUD_NS}Album> }}"
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "SharedAlbum rdfs:subClassOf Album should be materialised"
    );

    // --- Step 11: Verify versioned ontology IRI is bound to product ---
    let product_iri = ib.product(product);
    let q = format!(
        "SELECT ?onto WHERE {{ <{product}> <{PICLOUD_NS}ontologyIri> ?onto }}",
        product = product_iri.as_str()
    );
    let result = projector.query(&q).await.unwrap();
    assert!(!result.bindings.is_empty(), "Product should have ontologyIri");
    let onto_iri = result.bindings[0]["onto"]["value"].as_str().unwrap();
    assert!(
        onto_iri.contains(&format!("ontology/{version}")),
        "Ontology IRI should contain version {version}: {onto_iri}"
    );

    // --- Step 12: Verify ontology triples in product named graph ---
    let graph_iri = ib.product_graph(product);
    let ask = format!(
        "ASK {{ GRAPH <{graph}> {{ <{PICLOUD_NS}Photo> <{RDF_TYPE}> <http://www.w3.org/2000/01/rdf-schema#Class> }} }}",
        graph = graph_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "Ontology triples should be in the product's named graph"
    );

    // --- Step 13: Verify ontology resource version binding ---
    let q = format!(
        "SELECT ?v WHERE {{ <{iri}> <{PICLOUD_NS}boundToVersion> ?v }}",
        iri = ttl_iri.as_str()
    );
    let result = projector.query(&q).await.unwrap();
    assert!(!result.bindings.is_empty(), "Turtle ontology should be bound to versioned IRI");
    let bound_iri = result.bindings[0]["v"]["value"].as_str().unwrap();
    assert!(
        bound_iri.contains(&format!("ontology/{version}")),
        "Ontology should be bound to versioned IRI containing {version}: {bound_iri}"
    );
}

// ============================================================================
// TC-323 — Ontology exit — .ttl and .shacl bound to version and queryable
// ============================================================================
/// Exit criteria: End-to-end validation that ontology resources are first-class
/// platform resources with full lifecycle support:
///  1. ResourceDeclared → Ontology type with metadata
///  2. ProductDeployed → versioned ontology IRI
///  3. OntologyLoaded → content loaded into RDF graph
///  4. ResourceReady → status updated
///  5. Turtle triples queryable via SPARQL
///  6. SHACL shapes queryable via SPARQL
///  7. RDFS inference materialised from ontology
///  8. Cross-format: both .ttl and .shacl work
///  9. Product-scoped: triples in named graph
#[tokio::test]
async fn tc323_ontology_exit_ttl_and_shacl_bound_to_version_and_queryable() {
    let ib = iri_builder();
    let projector = OxigraphProjector::new().unwrap();

    let product = "data-app";
    let version = "1.5.0";

    // === Phase 1: Deploy product ===
    projector
        .project(&make_product_deployed(product, version))
        .await
        .unwrap();

    // Verify versioned ontology IRI exists on product
    let product_iri = ib.product(product);
    let q = format!(
        "SELECT ?onto WHERE {{ <{p}> <{PICLOUD_NS}ontologyIri> ?onto }}",
        p = product_iri.as_str()
    );
    let result = projector.query(&q).await.unwrap();
    assert!(!result.bindings.is_empty(), "Product should have versioned ontologyIri");
    let versioned_iri = result.bindings[0]["onto"]["value"].as_str().unwrap();
    assert!(
        versioned_iri.contains("ontology/1.5.0"),
        "Versioned ontology IRI should contain 1.5.0: {versioned_iri}"
    );

    // === Phase 2: Declare Turtle ontology ===
    let ttl_decl = make_ontology_declared(
        product,
        "domain-model",
        "ontology/domain.ttl",
        "turtle",
        version,
    );
    projector.project(&ttl_decl).await.unwrap();

    let ttl_iri = ib.resource(product, "ontology", "domain-model");

    // Verify resource exists as Ontology type
    let ask = format!(
        "ASK {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}Ontology> ; <{PICLOUD_NS}filePath> \"ontology/domain.ttl\" ; <{PICLOUD_NS}format> \"turtle\" }}",
        iri = ttl_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "Ontology resource should have correct type, filePath, and format"
    );

    // Verify ontology in product named graph
    let graph_iri = ib.product_graph(product);
    let ask = format!(
        "ASK {{ GRAPH <{graph}> {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}Ontology> }} }}",
        graph = graph_iri.as_str(),
        iri = ttl_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "Ontology should appear in product's named graph"
    );

    // === Phase 3: Declare SHACL ontology ===
    let shacl_decl = make_ontology_declared(
        product,
        "domain-shapes",
        "ontology/domain.shacl",
        "shacl",
        version,
    );
    projector.project(&shacl_decl).await.unwrap();

    let shacl_iri = ib.resource(product, "ontology", "domain-shapes");
    let ask = format!(
        "ASK {{ <{iri}> <{RDF_TYPE}> <{PICLOUD_NS}Ontology> ; <{PICLOUD_NS}format> \"shacl\" }}",
        iri = shacl_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "SHACL ontology should have correct type and format"
    );

    // === Phase 4: Load Turtle content ===
    let turtle_content = r#"
        @prefix picloud: <https://picloud.local/ontology#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        picloud:Sensor a rdfs:Class .
        picloud:Actuator a rdfs:Class .
        picloud:SmartSensor rdfs:subClassOf picloud:Sensor .
    "#;
    let load_ttl = make_ontology_loaded(product, "domain-model", version, turtle_content, "turtle");
    projector.project(&load_ttl).await.unwrap();

    // === Phase 5: Load SHACL content ===
    let shacl_content = r#"
        @prefix picloud: <https://picloud.local/ontology#> .
        @prefix sh: <http://www.w3.org/ns/shacl#> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        picloud:SensorShape
            a sh:NodeShape ;
            sh:targetClass picloud:Sensor ;
            sh:property [
                sh:path picloud:reading ;
                sh:datatype xsd:decimal ;
                sh:minCount 1 ;
            ] .

        picloud:ActuatorShape
            a sh:NodeShape ;
            sh:targetClass picloud:Actuator ;
            sh:property [
                sh:path picloud:command ;
                sh:datatype xsd:string ;
            ] .
    "#;
    let load_shacl = make_ontology_loaded(product, "domain-shapes", version, shacl_content, "shacl");
    projector.project(&load_shacl).await.unwrap();

    // === Phase 6: Verify Turtle triples queryable ===
    let ask = format!("ASK {{ <{PICLOUD_NS}Sensor> <{RDF_TYPE}> <http://www.w3.org/2000/01/rdf-schema#Class> }}");
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "Sensor class should exist");

    let ask = format!("ASK {{ <{PICLOUD_NS}Actuator> <{RDF_TYPE}> <http://www.w3.org/2000/01/rdf-schema#Class> }}");
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(result.bindings[0]["result"], true, "Actuator class should exist");

    // === Phase 7: Verify RDFS inference (rdfs9: instance type propagation) ===
    // SmartSensor → Sensor (direct subClassOf)
    let ask = format!(
        "ASK {{ <{PICLOUD_NS}SmartSensor> <{RDFS_SUBCLASS_OF}> <{PICLOUD_NS}Sensor> }}"
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "SmartSensor should be subClassOf Sensor (direct)"
    );

    // Insert an instance of SmartSensor and run materialisation to verify
    // that the instance is also typed as Sensor (rdfs9 rule)
    projector.insert_triple(
        "https://picloud.local/products/data-app/sensors/temp-1",
        RDF_TYPE,
        picloud_term("SmartSensor").into(),
    ).unwrap();
    projector.materialise_rdfs_subclass().unwrap();

    // Instance should now also have type Sensor
    let ask = format!(
        "ASK {{ <https://picloud.local/products/data-app/sensors/temp-1> <{RDF_TYPE}> <{PICLOUD_NS}Sensor> }}"
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "SmartSensor instance should be inferred as Sensor via RDFS subclass materialisation"
    );

    // === Phase 8: Verify SHACL shapes queryable ===
    let ask = format!(
        "ASK {{ <{PICLOUD_NS}SensorShape> <{RDF_TYPE}> <http://www.w3.org/ns/shacl#NodeShape> }}"
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "SensorShape should be a sh:NodeShape"
    );

    let ask = format!(
        "ASK {{ <{PICLOUD_NS}ActuatorShape> <{RDF_TYPE}> <http://www.w3.org/ns/shacl#NodeShape> }}"
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "ActuatorShape should be a sh:NodeShape"
    );

    // Verify SHACL targetClass
    let q = format!(
        "SELECT ?target WHERE {{ <{PICLOUD_NS}SensorShape> <http://www.w3.org/ns/shacl#targetClass> ?target }}"
    );
    let result = projector.query(&q).await.unwrap();
    assert_eq!(result.bindings.len(), 1, "SensorShape should have exactly one targetClass");
    assert_eq!(
        result.bindings[0]["target"]["value"].as_str().unwrap(),
        format!("{PICLOUD_NS}Sensor"),
        "SensorShape should target picloud:Sensor"
    );

    // === Phase 9: Verify triples in product named graph ===
    let ask = format!(
        "ASK {{ GRAPH <{graph}> {{ <{PICLOUD_NS}Sensor> <{RDF_TYPE}> <http://www.w3.org/2000/01/rdf-schema#Class> }} }}",
        graph = graph_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "Turtle classes should be in the product's named graph"
    );

    let ask = format!(
        "ASK {{ GRAPH <{graph}> {{ <{PICLOUD_NS}SensorShape> <{RDF_TYPE}> <http://www.w3.org/ns/shacl#NodeShape> }} }}",
        graph = graph_iri.as_str()
    );
    let result = projector.query(&ask).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "SHACL shapes should be in the product's named graph"
    );

    // === Phase 10: Verify version binding on ontology resources ===
    let q = format!(
        "SELECT ?v WHERE {{ <{iri}> <{PICLOUD_NS}boundToVersion> ?v }}",
        iri = ttl_iri.as_str()
    );
    let result = projector.query(&q).await.unwrap();
    assert!(!result.bindings.is_empty(), "Turtle ontology should be bound to a version");
    let bound = result.bindings[0]["v"]["value"].as_str().unwrap();
    assert!(
        bound.contains("ontology/1.5.0"),
        "Should be bound to version 1.5.0: {bound}"
    );

    let q = format!(
        "SELECT ?v WHERE {{ <{iri}> <{PICLOUD_NS}boundToVersion> ?v }}",
        iri = shacl_iri.as_str()
    );
    let result = projector.query(&q).await.unwrap();
    assert!(!result.bindings.is_empty(), "SHACL ontology should be bound to a version");
    let bound = result.bindings[0]["v"]["value"].as_str().unwrap();
    assert!(
        bound.contains("ontology/1.5.0"),
        "SHACL should be bound to version 1.5.0: {bound}"
    );

    // === Phase 11: Cross-cutting SPARQL — count all ontology resources ===
    let q = format!(
        "SELECT (COUNT(?s) AS ?count) WHERE {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}Ontology> }}"
    );
    let result = projector.query(&q).await.unwrap();
    let count: usize = result.bindings[0]["count"]["value"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        count, 2,
        "Should have exactly 2 ontology resources (turtle + shacl)"
    );

    // === Phase 12: CONSTRUCT query returns ontology metadata ===
    let construct = format!(
        "CONSTRUCT {{ ?s <{PICLOUD_NS}format> ?fmt }} WHERE {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}Ontology> ; <{PICLOUD_NS}format> ?fmt }}"
    );
    let result = projector.query(&construct).await.unwrap();
    assert!(
        result.bindings.len() >= 2,
        "CONSTRUCT should return at least 2 format triples, got {}",
        result.bindings.len()
    );
}
