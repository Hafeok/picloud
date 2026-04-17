/// FT-067 Integration Tests — Projection runner (trigger-driven CONSTRUCT + atomic swap)
///
/// Covers:
///   TC-198: data_product_projection_on_trigger
///   TC-200: data_product_atomic_swap
///
/// The projection runner subscribes to declared trigger events for each
/// data product and executes its SPARQL CONSTRUCT against the Product's
/// internal graph (ADR-056). The result atomically replaces the data
/// product's published named graph via an Oxigraph transaction so that
/// concurrent SPARQL queries never observe partial state.

use std::sync::Arc;
use std::time::Duration;

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::DataProductProjector;
use picloud_rdf::{
    DataProductProjectionRunner, DataProductRegistration, OxigraphDataProductProjector,
    OxigraphProjector, ProjectionOutcome,
};
use uuid::Uuid;

const PICLOUD_NS: &str = "https://picloud.local/ontology#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

fn iri_builder() -> IriBuilder {
    IriBuilder::new(ClusterDomain::default())
}

fn seed_photo(
    store: &oxigraph::store::Store,
    product_graph_iri: &str,
    photo_iri: &str,
    place: &str,
    lat: &str,
    lon: &str,
) {
    use oxigraph::model::{Literal, NamedNode, NamedNodeRef, QuadRef};
    store
        .insert_named_graph(NamedNodeRef::new(product_graph_iri).unwrap())
        .unwrap();
    let g = NamedNode::new(product_graph_iri).unwrap();
    let photo = NamedNode::new(photo_iri).unwrap();
    let type_pred = NamedNode::new(RDF_TYPE).unwrap();
    let photo_type = NamedNode::new(&format!("{PICLOUD_NS}Photo")).unwrap();
    store
        .insert(QuadRef::new(&photo, &type_pred, &photo_type, &g))
        .unwrap();
    store
        .insert(QuadRef::new(
            &photo,
            &NamedNode::new(&format!("{PICLOUD_NS}placeName")).unwrap(),
            &Literal::new_simple_literal(place),
            &g,
        ))
        .unwrap();
    store
        .insert(QuadRef::new(
            &photo,
            &NamedNode::new(&format!("{PICLOUD_NS}latitude")).unwrap(),
            &Literal::new_simple_literal(lat),
            &g,
        ))
        .unwrap();
    store
        .insert(QuadRef::new(
            &photo,
            &NamedNode::new(&format!("{PICLOUD_NS}longitude")).unwrap(),
            &Literal::new_simple_literal(lon),
            &g,
        ))
        .unwrap();
}

fn make_place_resolved(place: &str) -> EventEnvelope {
    let ib = iri_builder();
    EventEnvelope::new(
        ib.event_schema("PlaceResolved", 1),
        "PlaceResolved",
        ib.product("photo-app"),
        Some("photo-app".to_string()),
        Uuid::new_v4(),
        serde_json::json!({ "place": place }),
    )
}

fn construct_photo_locations(source_graph_iri: &str) -> String {
    format!(
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
        pg = source_graph_iri,
    )
}

// ============================================================================
// TC-198 — data_product_projection_on_trigger
// ============================================================================
/// Deploy `photo-app` with a data-product `photo-locations` declaring
/// `triggers: ['PlaceResolved']`. Emit a `PlaceResolved` event. Assert:
///   1. The SPARQL CONSTRUCT projection runs (triple_count > 0).
///   2. The data product named graph is populated with triples.
///   3. A `DataProductRefreshed` event is emitted with non-zero
///      triple_count, duration_ms, timestamp, and trigger_event.
///   4. The refresh timestamp is within the declared freshness.maxAge
///      window (i.e. "now" to within a reasonable delta).
///
/// Also asserts the push-only contract: emitting an unrelated event
/// after the first refresh does NOT run the projection again — only
/// declared trigger events cause rebuilds.
#[tokio::test]
async fn data_product_projection_on_trigger() {
    let ib = iri_builder();

    // --- Set up store with an operational graph seeded with photo data ---
    let projector = OxigraphProjector::new().unwrap();
    let product_graph_iri = ib.product_graph("photo-app");
    seed_photo(
        projector.store(),
        product_graph_iri.as_str(),
        "https://picloud.local/products/photo-app/photos/p1",
        "Paris",
        "48.8566",
        "2.3522",
    );
    seed_photo(
        projector.store(),
        product_graph_iri.as_str(),
        "https://picloud.local/products/photo-app/photos/p2",
        "London",
        "51.5074",
        "-0.1278",
    );

    // --- Wire up the data product projector + the trigger-driven runner ---
    let dp_projector_concrete =
        Arc::new(OxigraphDataProductProjector::new(Arc::new(projector.store().clone())));
    let runner = DataProductProjectionRunner::new(
        dp_projector_concrete.clone() as Arc<dyn DataProductProjector>,
    );

    let dp_graph_iri = ib.data_product_graph("photo-app", "photo-locations");
    let dp_resource_iri = dp_graph_iri.as_str().trim_end_matches("/graph").to_string();
    let dp_iri = ResourceIri::new(&dp_resource_iri).unwrap();

    let construct = construct_photo_locations(product_graph_iri.as_str());

    // The data product declares `triggers: ['PlaceResolved']` and
    // `freshness.maxAge = '15m'`. In the real platform, the registration
    // table is populated from DataProductDeclared events; here we
    // register directly to keep the test scoped to FT-067 behaviour.
    let max_age = Duration::from_secs(15 * 60);

    runner.register(DataProductRegistration::new(
        dp_iri.clone(),
        "photo-app",
        "photo-locations",
        vec!["PlaceResolved".to_string()],
        construct,
        product_graph_iri.clone(),
    ));

    // --- Pre-condition: the data product graph is currently empty ---
    let count_before = count_triples_in_graph(projector.store(), dp_graph_iri.as_str());
    assert_eq!(
        count_before, 0,
        "data product graph must start empty before the first trigger"
    );

    // --- Fire the trigger event. This simulates step 1 of ADR-056: ---
    //     "Trigger event arrives (e.g. PlaceResolved)"
    let before_refresh = chrono::Utc::now();
    let event = make_place_resolved("Paris");
    let outcomes = runner.handle_event(&event).await.unwrap();
    let after_refresh = chrono::Utc::now();

    // --- Assertion 1: exactly one projection was triggered ---
    assert_eq!(
        outcomes.len(),
        1,
        "one registered data product declared PlaceResolved as a trigger"
    );
    assert!(
        outcomes[0].is_refreshed(),
        "CONSTRUCT should succeed given a valid source graph"
    );

    // --- Assertion 2: DataProductRefreshed event payload ---
    let (event_out, triple_count, duration_ms) = match &outcomes[0] {
        ProjectionOutcome::Refreshed {
            event,
            triple_count,
            duration_ms,
        } => (event, *triple_count, *duration_ms),
        _ => unreachable!(),
    };
    assert_eq!(event_out.event_type, "DataProductRefreshed");
    assert_eq!(
        event_out.correlation_id, event.correlation_id,
        "refresh event must correlate with the trigger that caused it"
    );
    assert_eq!(event_out.product.as_deref(), Some("photo-app"));
    assert_eq!(
        event_out.source.as_str(),
        dp_iri.as_str(),
        "refresh event source should be the data product IRI"
    );
    assert_eq!(event_out.payload["trigger_event"], "PlaceResolved");
    assert_eq!(event_out.payload["data_product_iri"], dp_iri.as_str());

    // --- Assertion 3: non-zero triple count ---
    // 2 photos x 3 projected properties = 6 triples
    assert_eq!(
        triple_count, 6,
        "CONSTRUCT should project 6 triples (2 photos x 3 properties)"
    );
    assert_eq!(event_out.payload["triple_count"], 6);
    assert!(triple_count > 0, "triple_count must be non-zero");

    // --- Assertion 4: duration is recorded (sanity-check only — this is ---
    //     a small in-memory CONSTRUCT, so it should finish in a few ms) ---
    assert!(
        duration_ms < 5_000,
        "duration_ms should be < 5s for a tiny projection, got {duration_ms}ms"
    );
    assert_eq!(event_out.payload["duration_ms"], duration_ms);

    // --- Assertion 5: refreshed_at timestamp is within the freshness.maxAge ---
    //     window relative to the trigger time (a just-successful refresh is,
    //     by construction, fresh — staleness cannot exceed `now() - before`). ---
    let refreshed_at_str = event_out.payload["refreshed_at"]
        .as_str()
        .expect("refreshed_at should be an ISO-8601 string in the event payload");
    let refreshed_at = chrono::DateTime::parse_from_rfc3339(refreshed_at_str)
        .expect("refreshed_at should parse as RFC-3339")
        .with_timezone(&chrono::Utc);
    assert!(
        refreshed_at >= before_refresh - chrono::Duration::seconds(1),
        "refreshed_at ({refreshed_at}) should be >= pre-trigger time ({before_refresh})"
    );
    assert!(
        refreshed_at <= after_refresh + chrono::Duration::seconds(1),
        "refreshed_at ({refreshed_at}) should be <= post-trigger time ({after_refresh})"
    );
    let age = chrono::Utc::now().signed_duration_since(refreshed_at);
    assert!(
        age.to_std().unwrap_or(Duration::from_secs(0)) < max_age,
        "refreshed_at must be within freshness.maxAge of now — \
         age = {:?}, maxAge = {:?}",
        age,
        max_age
    );

    // --- Assertion 6: the data product named graph is populated ---
    let count_after = count_triples_in_graph(projector.store(), dp_graph_iri.as_str());
    assert_eq!(
        count_after, 6,
        "data product graph must contain exactly the CONSTRUCT output after refresh"
    );

    // Confirm the actual triples are present with correct values via
    // the projector's own query_data_product API (the same one a
    // consumer product would use).
    let rows = dp_projector_concrete
        .query_data_product(
            &dp_iri,
            &format!("?photo <{PICLOUD_NS}placeName> ?place ; <{PICLOUD_NS}latitude> ?lat"),
        )
        .await
        .unwrap();
    assert_eq!(rows.bindings.len(), 2, "2 photos projected");
    let mut places: Vec<String> = rows
        .bindings
        .iter()
        .map(|b| b["place"].as_str().unwrap_or("").to_string())
        .collect();
    places.sort();
    assert_eq!(places, vec!["London".to_string(), "Paris".to_string()]);

    // --- Assertion 7: push-only semantics — non-trigger events do NOT ---
    //     rebuild the projection. Emit something unrelated and verify no ---
    //     outcomes come back. ---
    let irrelevant = EventEnvelope::new(
        ib.event_schema("PhotoLiked", 1),
        "PhotoLiked",
        ib.product("photo-app"),
        Some("photo-app".to_string()),
        Uuid::new_v4(),
        serde_json::json!({ "photo_id": "p1" }),
    );
    let none = runner.handle_event(&irrelevant).await.unwrap();
    assert!(
        none.is_empty(),
        "non-trigger events must NOT run projections (push-only model)"
    );
}

// ============================================================================
// TC-200 — data_product_atomic_swap
// ============================================================================
/// Trigger a projection rebuild while a consumer (maps-app) is issuing
/// SPARQL queries against the data product graph at ~20 queries/second.
///
/// Assertions:
///   1. Zero query errors during the swap.
///   2. No query returns a mix of triples from the old and new projections.
///      Equivalently: every query observes either the old state (pre-swap)
///      or the new state (post-swap), never a half-written state.
///   3. Multiple rebuilds during the query burst all succeed atomically.
///
/// The contention model: we spawn one task that hammers the data product
/// graph with SPARQL queries for the duration of the test, and another task
/// that issues several trigger events that each cause a CONSTRUCT + atomic
/// swap against the same graph.
#[tokio::test]
async fn data_product_atomic_swap() {
    let ib = iri_builder();

    // --- Build the store, seed with 3 photos, and wire up the runner ---
    let projector = OxigraphProjector::new().unwrap();
    let store_arc = Arc::new(projector.store().clone());
    let product_graph_iri = ib.product_graph("photo-app");

    // Seed three photos for the *old* projection (3 photos x 3 triples = 9).
    seed_photo(
        &store_arc,
        product_graph_iri.as_str(),
        "https://picloud.local/products/photo-app/photos/p1",
        "Paris",
        "48.8566",
        "2.3522",
    );
    seed_photo(
        &store_arc,
        product_graph_iri.as_str(),
        "https://picloud.local/products/photo-app/photos/p2",
        "London",
        "51.5074",
        "-0.1278",
    );
    seed_photo(
        &store_arc,
        product_graph_iri.as_str(),
        "https://picloud.local/products/photo-app/photos/p3",
        "Tokyo",
        "35.6762",
        "139.6503",
    );

    let dp_projector_concrete = Arc::new(OxigraphDataProductProjector::new(store_arc.clone()));
    let runner = Arc::new(DataProductProjectionRunner::new(
        dp_projector_concrete.clone() as Arc<dyn DataProductProjector>,
    ));

    let dp_graph_iri = ib.data_product_graph("photo-app", "photo-locations");
    let dp_graph_str = dp_graph_iri.as_str().to_string();
    let dp_resource_iri = dp_graph_iri.as_str().trim_end_matches("/graph").to_string();
    let dp_iri = ResourceIri::new(&dp_resource_iri).unwrap();

    let construct = construct_photo_locations(product_graph_iri.as_str());
    runner.register(DataProductRegistration::new(
        dp_iri.clone(),
        "photo-app",
        "photo-locations",
        vec!["PlaceResolved".to_string()],
        construct,
        product_graph_iri.clone(),
    ));

    // --- First refresh so the graph has the OLD state (9 triples) ---
    let first = runner.handle_event(&make_place_resolved("initial")).await.unwrap();
    assert_eq!(first.len(), 1);
    assert!(first[0].is_refreshed());
    assert_eq!(count_triples_in_graph(&store_arc, &dp_graph_str), 9);

    // --- Spawn the query hammer task — 20 queries/sec for ~1 second ---
    // The query counts how many triples are in the data product graph.
    // A mid-swap partial read would return something OTHER than the
    // expected counts (0, 9, or the new 6); we assert every observation
    // is one of those values. Zero errors is asserted implicitly via
    // `.unwrap()` — an error would panic the task.
    let query_store = store_arc.clone();
    let query_graph = dp_graph_str.clone();
    let total_queries = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let errors = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let partial_reads = Arc::new(std::sync::atomic::AtomicU64::new(0));

    // Valid snapshot triple counts: the initial projection has 9 triples
    // (3 photos x 3 triples each); after the rebuild it has 6 triples
    // (2 photos x 3 triples each). No other value is an acceptable
    // snapshot — any other would indicate partial state.
    let valid_counts: Vec<u64> = vec![9, 6];

    let total_queries_c = total_queries.clone();
    let errors_c = errors.clone();
    let partial_reads_c = partial_reads.clone();
    let valid_counts_c = valid_counts.clone();
    let query_task = tokio::spawn(async move {
        let query_sparql = format!(
            "SELECT (COUNT(*) AS ?n) WHERE {{ GRAPH <{qg}> {{ ?s ?p ?o }} }}",
            qg = query_graph
        );
        let deadline = tokio::time::Instant::now() + Duration::from_millis(1_000);
        let mut tick = tokio::time::interval(Duration::from_millis(50));
        while tokio::time::Instant::now() < deadline {
            tick.tick().await;
            match query_store.query(query_sparql.as_str()) {
                Ok(oxigraph::sparql::QueryResults::Solutions(solutions)) => {
                    for sol in solutions {
                        match sol {
                            Ok(s) => {
                                total_queries_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                if let Some(term) = s.get("n") {
                                    if let oxigraph::model::Term::Literal(lit) = term {
                                        let n: u64 =
                                            lit.value().parse().unwrap_or(u64::MAX);
                                        if !valid_counts_c.contains(&n) {
                                            partial_reads_c.fetch_add(
                                                1,
                                                std::sync::atomic::Ordering::SeqCst,
                                            );
                                        }
                                    }
                                }
                            }
                            Err(_) => {
                                errors_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            }
                        }
                    }
                }
                Ok(_) => {
                    errors_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
                Err(_) => {
                    errors_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
        }
    });

    // --- While queries are flying, mutate the source graph to shrink the
    //     projection from 9 triples (3 photos) to 6 triples (2 photos),
    //     then re-trigger. Re-trigger multiple times to cross-check repeated
    //     atomic swaps never expose partial state. ---
    // First: remove p3 (Tokyo) from the source graph.
    {
        use oxigraph::model::{GraphNameRef, NamedNode};
        let g = NamedNode::new(product_graph_iri.as_str()).unwrap();
        let photo3 =
            NamedNode::new("https://picloud.local/products/photo-app/photos/p3").unwrap();
        let quads: Vec<_> = store_arc
            .quads_for_pattern(
                Some((&photo3).into()),
                None,
                None,
                Some(GraphNameRef::from(&g)),
            )
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        for q in &quads {
            store_arc.remove(q).unwrap();
        }
    }

    // Stagger a few trigger events so the swap happens in the middle of
    // the query hammer. 5 refreshes over ~500ms means 1 swap every ~100ms
    // while queries run every ~50ms — plenty of contention.
    let runner_c = runner.clone();
    let refresh_task = tokio::spawn(async move {
        let mut refreshed_count = 0u64;
        for _ in 0..5 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let outcomes = runner_c
                .handle_event(&make_place_resolved("swap"))
                .await
                .unwrap();
            assert_eq!(outcomes.len(), 1);
            if let ProjectionOutcome::Refreshed { triple_count, .. } = &outcomes[0] {
                // After p3 removed, the CONSTRUCT returns 6 triples.
                assert_eq!(*triple_count, 6);
                refreshed_count += 1;
            } else {
                panic!("refresh must succeed during atomic swap test");
            }
        }
        refreshed_count
    });

    let refresh_count = refresh_task.await.unwrap();
    query_task.await.unwrap();

    assert_eq!(refresh_count, 5, "all 5 refreshes should have succeeded");

    // --- Assertion 1: zero query errors ---
    let total_err = errors.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        total_err, 0,
        "no query should error during atomic swap, got {total_err}"
    );

    // --- Assertion 2: no partial-state reads ---
    let partial = partial_reads.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        partial, 0,
        "no query should observe a mix of old and new triples — got {partial} partial reads"
    );

    // --- Assertion 3: the final state is the new projection (6 triples) ---
    let final_count = count_triples_in_graph(&store_arc, &dp_graph_str);
    assert_eq!(
        final_count, 6,
        "final data product graph must reflect the latest CONSTRUCT (6 triples)"
    );

    // --- Assertion 4: we actually ran a meaningful number of queries ---
    // (sanity — the hammer task should have issued ~20 queries in 1s at
    // 50ms intervals, though scheduler jitter may vary). We require at
    // least 10 to ensure the contention window was real.
    let issued = total_queries.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        issued >= 10,
        "expected at least 10 queries during the 1s window, got {issued}"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Count all quads in a named graph using Oxigraph's direct quad iterator
/// (avoids any SPARQL parsing overhead and matches the data the transaction
/// snapshot exposes).
fn count_triples_in_graph(store: &oxigraph::store::Store, graph_iri: &str) -> u64 {
    use oxigraph::model::{GraphNameRef, NamedNode};
    let g = NamedNode::new(graph_iri).unwrap();
    store
        .quads_for_pattern(None, None, None, Some(GraphNameRef::from(&g)))
        .filter_map(|q| q.ok())
        .count() as u64
}
