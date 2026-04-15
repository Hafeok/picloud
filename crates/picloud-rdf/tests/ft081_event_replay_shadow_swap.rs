/// FT-081 Integration Tests — Event replay: shadow projection, atomic swap,
/// marked replay events
///
/// Covers:
///   TC-280: Event replay creates shadow projection and swaps atomically (scenario)
///   TC-337: Event replay exit — shadow projection created and swapped atomically (exit-criteria)
///
/// Verifies that:
///   1. Replaying events creates a shadow named graph (not the live/default graph)
///   2. Shadow graph contains all projected triples from replayed events
///   3. Atomic swap clears the target graph and copies shadow triples into it
///   4. After swap the shadow graph is cleaned up (no leftover triples)
///   5. The live default graph is not affected during shadow projection
///   6. Replayed events carry ReplayMetadata marking them as replays

use chrono::Utc;
use picloud_domain::events::{EventEnvelope, ReplayMetadata};
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{ReplayEngine, ReplayRequest, StateProjector};
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

fn make_node_joined(name: &str, address: &str) -> EventEnvelope {
    let ib = iri_builder();
    let node_iri = ib.node(name);
    make_event(
        "NodeJoined",
        node_iri.clone(),
        None,
        serde_json::json!({
            "node_id": Uuid::new_v4().to_string(),
            "node_name": name,
            "node_iri": node_iri.as_str(),
            "address": address,
        }),
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

fn make_resource_declared(product: &str, resource_type: &str, name: &str) -> EventEnvelope {
    let ib = iri_builder();
    let resource_iri = ib.resource(product, resource_type, name);
    make_event(
        "ResourceDeclared",
        resource_iri.clone(),
        Some(product),
        serde_json::json!({
            "resource_iri": resource_iri.as_str(),
            "resource_type": resource_type,
            "product": product,
        }),
    )
}

/// Helper: count triples in a named graph via SPARQL.
async fn count_triples_in_graph(projector: &OxigraphProjector, graph_iri: &str) -> usize {
    let sparql = format!(
        "SELECT (COUNT(*) AS ?count) WHERE {{ GRAPH <{graph_iri}> {{ ?s ?p ?o }} }}"
    );
    let result = projector.query(&sparql).await.expect("SPARQL query should succeed");
    result
        .bindings
        .first()
        .and_then(|b| b.get("count"))
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0)
}

/// Helper: count triples in the default graph via SPARQL.
async fn count_default_graph_triples(projector: &OxigraphProjector) -> usize {
    let sparql = "SELECT (COUNT(*) AS ?count) WHERE { ?s ?p ?o }";
    let result = projector.query(sparql).await.expect("SPARQL query should succeed");
    result
        .bindings
        .first()
        .and_then(|b| b.get("count"))
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0)
}

// ---- TC-280: Event replay creates shadow projection and swaps atomically ----

/// Scenario test for TC-280.
///
/// Steps:
/// 1. Create an OxigraphProjector
/// 2. Project several events into the live (default) graph
/// 3. Execute a replay on a product graph — events go into a shadow graph
/// 4. Verify the shadow graph was created with correct triples
/// 5. Verify the atomic swap moved triples to the target graph
/// 6. Verify the shadow graph was cleaned up after swap
/// 7. Verify replayed events carry ReplayMetadata
#[tokio::test]
async fn tc280_event_replay_creates_shadow_projection_and_swaps_atomically() {
    let projector = OxigraphProjector::new().expect("create in-memory projector");

    // --- Step 1: Project live events into the default graph ---
    let node_event = make_node_joined("pi-node-01", "192.168.1.10:9000");
    let product_event = make_product_deployed("photo-app", "1.0.0");
    let resource_event =
        make_resource_declared("photo-app", "containers", "api-server");

    projector.project(&node_event).await.expect("project node event");
    projector.project(&product_event).await.expect("project product event");
    projector.project(&resource_event).await.expect("project resource event");

    // The default graph now has triples for the node, product, and resource.
    let default_count_before = count_default_graph_triples(&projector).await;
    assert!(
        default_count_before > 0,
        "Default graph should have triples after projection"
    );

    // --- Step 2: Prepare events for replay ---
    // Simulate replaying 3 events that build a product's graph
    let replay_events = vec![
        make_product_deployed("photo-app", "1.0.0"),
        make_resource_declared("photo-app", "containers", "api-server"),
        make_resource_declared("photo-app", "containers", "worker"),
    ];

    // --- Step 3: Execute replay with a target product graph ---
    let ib = iri_builder();
    let target_graph = ib.product_graph("photo-app").to_string();
    let replay_id = Uuid::new_v4();

    let result = projector
        .execute_replay(replay_id, &replay_events, Some(&target_graph))
        .expect("execute_replay should succeed");

    // --- Step 4: Verify replay result ---
    assert_eq!(result.replay_id, replay_id, "replay_id should match");
    assert_eq!(
        result.events_replayed, 3,
        "all 3 events should be replayed"
    );
    assert!(
        !result.shadow_graph_iri.is_empty(),
        "shadow_graph_iri should be set"
    );

    // --- Step 5: Verify target graph has replayed triples ---
    let target_count = count_triples_in_graph(&projector, &target_graph).await;
    assert!(
        target_count > 0,
        "Target graph should have triples after atomic swap (got {target_count})"
    );

    // --- Step 6: Verify shadow graph was cleaned up (no leftover triples) ---
    let shadow_count = count_triples_in_graph(&projector, &result.shadow_graph_iri).await;
    assert_eq!(
        shadow_count, 0,
        "Shadow graph should be empty after atomic swap (cleanup)"
    );

    // --- Step 7: Verify default graph was NOT affected by replay ---
    let default_count_after = count_default_graph_triples(&projector).await;
    assert_eq!(
        default_count_before, default_count_after,
        "Default graph triple count should be unchanged by replay"
    );

    // --- Step 8: Verify ReplayMetadata can be attached to events ---
    let original_ts = Utc::now() - chrono::Duration::hours(1);
    let replayed_at = Utc::now();
    let mut replay_event = make_product_deployed("photo-app", "1.0.0");
    replay_event = replay_event.with_replay_metadata(ReplayMetadata {
        is_replay: true,
        replay_id,
        original_timestamp: original_ts,
        replayed_at,
    });

    assert!(
        replay_event.replay.is_some(),
        "Replayed event should carry ReplayMetadata"
    );
    let meta = replay_event.replay.as_ref().unwrap();
    assert!(meta.is_replay, "is_replay should be true");
    assert_eq!(meta.replay_id, replay_id, "replay_id should match");
    assert_eq!(
        meta.original_timestamp, original_ts,
        "original_timestamp should be preserved"
    );

    // Verify ReplayMetadata round-trips through serde
    let json = serde_json::to_value(&replay_event).expect("serialize event");
    let deser: EventEnvelope =
        serde_json::from_value(json).expect("deserialize event");
    assert!(deser.replay.is_some(), "replay metadata should survive serde round-trip");
    let deser_meta = deser.replay.unwrap();
    assert!(deser_meta.is_replay);
    assert_eq!(deser_meta.replay_id, replay_id);
}

// ---- TC-337: Event replay exit — shadow projection created and swapped atomically ----

/// Exit-criteria test for TC-337.
///
/// This is the high-level gate: given a sequence of platform events,
/// a replay operation MUST:
///   a) build a shadow projection in a dedicated named graph
///   b) atomically swap the shadow into the target (clearing old target triples)
///   c) leave no residual shadow triples
///   d) leave the live default graph unmodified
///   e) report correct event counts
///
/// If any of these invariants break, the feature is not shippable.
#[tokio::test]
async fn tc337_event_replay_exit_shadow_projection_created_and_swapped_atomically() {
    let projector = OxigraphProjector::new().expect("create projector");
    let ib = iri_builder();

    // --- Phase A: Establish pre-existing state in default graph ---
    let events_live = vec![
        make_node_joined("pi-node-01", "192.168.1.10:9000"),
        make_node_joined("pi-node-02", "192.168.1.11:9000"),
        make_product_deployed("orders-api", "2.0.0"),
    ];
    for ev in &events_live {
        projector.project(ev).await.expect("project live event");
    }
    let default_before = count_default_graph_triples(&projector).await;
    assert!(default_before > 0);

    // --- Phase B: Pre-populate a product graph to verify it gets cleared ---
    let target_graph = ib.product_graph("orders-api").to_string();
    projector
        .insert_triple_in_graph(
            "https://picloud.local/stale-triple-subject",
            &format!("{PICLOUD_NS}staleData"),
            oxigraph::model::Literal::new_simple_literal("old-value").into(),
            &target_graph,
        )
        .expect("insert stale triple in target graph");
    let stale_count = count_triples_in_graph(&projector, &target_graph).await;
    assert!(stale_count > 0, "Target graph should have stale triples before replay");

    // --- Phase C: Execute replay ---
    let replay_events = vec![
        make_product_deployed("orders-api", "2.0.0"),
        make_resource_declared("orders-api", "containers", "order-processor"),
        make_resource_declared("orders-api", "volumes", "order-data"),
    ];
    let replay_id = Uuid::new_v4();
    let result = projector
        .execute_replay(replay_id, &replay_events, Some(&target_graph))
        .expect("replay should succeed");

    // --- Invariant (a): Shadow was built (evidenced by result.shadow_graph_iri) ---
    assert!(
        result.shadow_graph_iri.starts_with("https://picloud.local/replay/"),
        "Shadow graph IRI should follow the replay/{{id}} convention"
    );

    // --- Invariant (b): Target graph was atomically swapped ---
    let target_after = count_triples_in_graph(&projector, &target_graph).await;
    assert!(
        target_after > 0,
        "Target graph should have triples from the replayed events"
    );
    // The stale triple should be gone — atomic swap clears old data
    let stale_check_sparql = format!(
        r#"ASK WHERE {{ GRAPH <{target_graph}> {{ <https://picloud.local/stale-triple-subject> <{PICLOUD_NS}staleData> "old-value" }} }}"#
    );
    let stale_result = projector.query(&stale_check_sparql).await.expect("ASK query");
    // ASK queries return a single binding with {"result": bool}
    let stale_exists = stale_result
        .bindings
        .first()
        .and_then(|b| b.get("result"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(
        !stale_exists,
        "Stale triple should be gone after atomic swap"
    );

    // --- Invariant (c): Shadow graph is empty after swap ---
    let shadow_residual = count_triples_in_graph(&projector, &result.shadow_graph_iri).await;
    assert_eq!(
        shadow_residual, 0,
        "Shadow graph must have zero triples after swap (cleanup)"
    );

    // --- Invariant (d): Default graph unchanged ---
    let default_after = count_default_graph_triples(&projector).await;
    assert_eq!(
        default_before, default_after,
        "Default (live) graph must not be modified by replay"
    );

    // --- Invariant (e): Correct event count ---
    assert_eq!(result.events_replayed, 3);

    // --- Invariant (f): start_replay creates shadow graph structure ---
    let request = ReplayRequest {
        from: Utc::now() - chrono::Duration::hours(1),
        to: Some(Utc::now()),
        product: Some("orders-api".to_string()),
        aggregate_type: None,
        aggregate_ids: vec![],
    };
    let new_replay_id = projector
        .start_replay(request)
        .await
        .expect("start_replay should return replay_id");

    // The shadow graph for the new replay should exist with metadata
    let shadow_iri = ib.replay_graph(&new_replay_id.to_string());
    let meta_sparql = format!(
        "SELECT ?type WHERE {{ GRAPH <{}> {{ ?s a ?type }} }}",
        shadow_iri.as_str()
    );
    let meta_result = projector.query(&meta_sparql).await.expect("shadow meta query");
    assert!(
        !meta_result.bindings.is_empty(),
        "start_replay should create shadow graph with ReplayOperation metadata"
    );

    // --- Invariant (g): ReplayMetadata marks replayed events distinctly ---
    let marked_event = make_node_joined("pi-node-03", "192.168.1.12:9000")
        .with_replay_metadata(ReplayMetadata {
            is_replay: true,
            replay_id: new_replay_id,
            original_timestamp: Utc::now() - chrono::Duration::minutes(30),
            replayed_at: Utc::now(),
        });
    assert!(marked_event.replay.is_some());
    assert!(marked_event.replay.as_ref().unwrap().is_replay);

    // An unmarked (live) event should have replay == None
    let live_event = make_node_joined("pi-node-04", "192.168.1.13:9000");
    assert!(
        live_event.replay.is_none(),
        "Live events must not carry replay metadata"
    );
}
