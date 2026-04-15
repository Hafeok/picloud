/// FT-082 Integration Tests — Aggregate-scoped replay (single and batch up to 1000)
///
/// Covers:
///   TC-281: Aggregate-scoped replay replays events for single aggregate (scenario)
///   TC-338: Scoped replay exit — single aggregate replay completes (exit-criteria)
///
/// Verifies that:
///   1. When a replay request specifies aggregate_type and aggregate_ids,
///      only events matching those aggregates are replayed into the shadow graph
///   2. Events for other aggregates are excluded from the shadow projection
///   3. Single aggregate replay (one ID) works end-to-end
///   4. Batch aggregate replay (multiple IDs, up to 1000) works end-to-end
///   5. start_replay records aggregate scope metadata in the shadow graph
///   6. The shadow graph is cleaned up after atomic swap
///   7. Batch size > 1000 is rejected with a validation error

use chrono::Utc;
use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder};
use picloud_domain::traits::{ReplayEngine, ReplayRequest, StateProjector};
use picloud_rdf::OxigraphProjector;
use uuid::Uuid;

const PICLOUD_NS: &str = "https://picloud.local/ontology#";

fn iri_builder() -> IriBuilder {
    IriBuilder::new(ClusterDomain::default())
}

/// Create a ProductEventAppended event for a specific aggregate.
fn make_product_event(
    product: &str,
    store_name: &str,
    aggregate_type: &str,
    aggregate_id: &str,
    event_type: &str,
    data: serde_json::Value,
) -> EventEnvelope {
    let ib = iri_builder();
    let event_iri = ib.aggregate_stream(product, store_name, aggregate_type, aggregate_id);
    let source = ib.product(product);
    EventEnvelope::new(
        ib.event_schema("ProductEventAppended", 1),
        "ProductEventAppended",
        source,
        Some(product.to_string()),
        Uuid::new_v4(),
        serde_json::json!({
            "event_iri": event_iri.as_str(),
            "aggregate_type": aggregate_type,
            "aggregate_id": aggregate_id,
            "product_event_type": event_type,
            "store_name": store_name,
            "data": data,
        }),
    )
}

/// Create a generic event (not aggregate-scoped) for a product.
fn make_product_deployed(name: &str, version: &str) -> EventEnvelope {
    let ib = iri_builder();
    let product_iri = ib.product(name);
    EventEnvelope::new(
        ib.event_schema("ProductDeployed", 1),
        "ProductDeployed",
        product_iri.clone(),
        Some(name.to_string()),
        Uuid::new_v4(),
        serde_json::json!({
            "product_iri": product_iri.as_str(),
            "product_name": name,
            "version": version,
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

// ---- TC-281: Aggregate-scoped replay replays events for single aggregate ----

/// Scenario test for TC-281.
///
/// Steps:
/// 1. Create an OxigraphProjector
/// 2. Build events for multiple aggregates: Order-001, Order-002, Order-003
/// 3. Execute an aggregate-scoped replay targeting only Order-001
/// 4. Verify only Order-001 events appear in the shadow/target graph
/// 5. Verify Order-002 and Order-003 events are NOT in the target graph
/// 6. Verify the event count matches only Order-001 events
/// 7. Verify batch replay with multiple IDs works
/// 8. Verify batch > 1000 is rejected
#[tokio::test]
async fn tc281_aggregate_scoped_replay_replays_events_for_single_aggregate() {
    let projector = OxigraphProjector::new().expect("create in-memory projector");

    // --- Step 1: Build events for multiple aggregates ---
    let now = Utc::now();
    let product = "orders-app";

    // Order-001: 3 events
    let order_001_events = vec![
        make_product_event(product, "main", "Order", "order-001", "OrderCreated",
            serde_json::json!({"customer": "alice", "total": 100})),
        make_product_event(product, "main", "Order", "order-001", "OrderPaid",
            serde_json::json!({"payment_method": "card"})),
        make_product_event(product, "main", "Order", "order-001", "OrderShipped",
            serde_json::json!({"tracking": "TRK-001"})),
    ];

    // Order-002: 2 events
    let order_002_events = vec![
        make_product_event(product, "main", "Order", "order-002", "OrderCreated",
            serde_json::json!({"customer": "bob", "total": 200})),
        make_product_event(product, "main", "Order", "order-002", "OrderPaid",
            serde_json::json!({"payment_method": "paypal"})),
    ];

    // Order-003: 1 event (different aggregate type)
    let invoice_events = vec![
        make_product_event(product, "main", "Invoice", "inv-001", "InvoiceIssued",
            serde_json::json!({"amount": 300})),
    ];

    // Project all events into default graph first (simulates live state)
    let all_events: Vec<EventEnvelope> = order_001_events.iter()
        .chain(order_002_events.iter())
        .chain(invoice_events.iter())
        .cloned()
        .collect();

    for ev in &all_events {
        projector.project(ev).await.expect("project event");
    }

    let default_before = count_default_graph_triples(&projector).await;
    assert!(default_before > 0, "Default graph should have triples after projection");

    // --- Step 2: Execute aggregate-scoped replay for single aggregate (Order-001) ---
    let ib = iri_builder();
    let target_graph = ib.product_graph(product).to_string();
    let replay_id = Uuid::new_v4();

    let request = ReplayRequest {
        from: now - chrono::Duration::hours(1),
        to: Some(now + chrono::Duration::hours(1)),
        product: Some(product.to_string()),
        aggregate_type: Some("Order".to_string()),
        aggregate_ids: vec!["order-001".to_string()],
    };

    let result = projector
        .execute_aggregate_replay(replay_id, &all_events, &request, Some(&target_graph))
        .expect("aggregate replay should succeed");

    // --- Step 3: Verify only Order-001 events were replayed ---
    assert_eq!(result.replay_id, replay_id, "replay_id should match");
    assert_eq!(
        result.events_replayed, 3,
        "Only the 3 Order-001 events should be replayed (got {})",
        result.events_replayed
    );

    // --- Step 4: Verify target graph has triples ---
    let target_count = count_triples_in_graph(&projector, &target_graph).await;
    assert!(
        target_count > 0,
        "Target graph should have triples from replayed Order-001 events"
    );

    // --- Step 5: Verify shadow graph was cleaned up ---
    let shadow_count = count_triples_in_graph(&projector, &result.shadow_graph_iri).await;
    assert_eq!(
        shadow_count, 0,
        "Shadow graph should be empty after atomic swap"
    );

    // --- Step 6: Verify default graph unchanged ---
    let default_after = count_default_graph_triples(&projector).await;
    assert_eq!(
        default_before, default_after,
        "Default graph should be unchanged by aggregate replay"
    );

    // --- Step 7: Batch replay with multiple aggregate IDs ---
    let replay_id_batch = Uuid::new_v4();
    let batch_request = ReplayRequest {
        from: now - chrono::Duration::hours(1),
        to: Some(now + chrono::Duration::hours(1)),
        product: Some(product.to_string()),
        aggregate_type: Some("Order".to_string()),
        aggregate_ids: vec!["order-001".to_string(), "order-002".to_string()],
    };

    let batch_result = projector
        .execute_aggregate_replay(replay_id_batch, &all_events, &batch_request, None)
        .expect("batch aggregate replay should succeed");

    assert_eq!(
        batch_result.events_replayed, 5,
        "Batch replay should include 3 Order-001 + 2 Order-002 = 5 events (got {})",
        batch_result.events_replayed
    );

    // --- Step 8: Verify batch > 1000 is rejected ---
    let oversized_ids: Vec<String> = (0..1001).map(|i| format!("order-{i}")).collect();
    let oversized_request = ReplayRequest {
        from: now - chrono::Duration::hours(1),
        to: Some(now + chrono::Duration::hours(1)),
        product: Some(product.to_string()),
        aggregate_type: Some("Order".to_string()),
        aggregate_ids: oversized_ids,
    };

    let oversized_result = projector
        .execute_aggregate_replay(Uuid::new_v4(), &all_events, &oversized_request, None);
    assert!(
        oversized_result.is_err(),
        "Batch replay with > 1000 IDs should be rejected"
    );

    // --- Step 9: Aggregate type filter only (no specific IDs) ---
    let type_only_request = ReplayRequest {
        from: now - chrono::Duration::hours(1),
        to: Some(now + chrono::Duration::hours(1)),
        product: Some(product.to_string()),
        aggregate_type: Some("Order".to_string()),
        aggregate_ids: vec![],
    };

    let type_only_result = projector
        .execute_aggregate_replay(Uuid::new_v4(), &all_events, &type_only_request, None)
        .expect("type-only aggregate replay should succeed");

    assert_eq!(
        type_only_result.events_replayed, 5,
        "Type-only replay should include all 5 Order events (got {})",
        type_only_result.events_replayed
    );
}

// ---- TC-338: Scoped replay exit — single aggregate replay completes ----

/// Exit-criteria test for TC-338.
///
/// This is the high-level gate: given an aggregate-scoped replay request,
/// the replay operation MUST:
///   a) Filter events to only the target aggregate
///   b) Build a shadow projection containing only filtered events
///   c) Atomically swap shadow into target graph
///   d) Record aggregate scope metadata in the shadow graph (via start_replay)
///   e) Report correct (filtered) event count
///   f) Clean up the shadow graph after swap
///   g) Leave default graph unmodified
///
/// If any of these invariants break, the feature is not shippable.
#[tokio::test]
async fn tc338_scoped_replay_exit_single_aggregate_replay_completes() {
    let projector = OxigraphProjector::new().expect("create projector");
    let ib = iri_builder();
    let now = Utc::now();
    let product = "inventory-app";

    // --- Phase A: Create events for multiple aggregates ---
    let sku_001_events = vec![
        make_product_event(product, "main", "SKU", "sku-001", "StockReceived",
            serde_json::json!({"quantity": 100, "warehouse": "east"})),
        make_product_event(product, "main", "SKU", "sku-001", "StockReserved",
            serde_json::json!({"quantity": 10, "order": "ord-100"})),
        make_product_event(product, "main", "SKU", "sku-001", "StockShipped",
            serde_json::json!({"quantity": 10, "order": "ord-100"})),
    ];

    let sku_002_events = vec![
        make_product_event(product, "main", "SKU", "sku-002", "StockReceived",
            serde_json::json!({"quantity": 50, "warehouse": "west"})),
        make_product_event(product, "main", "SKU", "sku-002", "StockReserved",
            serde_json::json!({"quantity": 5, "order": "ord-101"})),
    ];

    // A non-aggregate event for the product (ProductDeployed)
    let deploy_event = make_product_deployed(product, "3.0.0");

    let all_events: Vec<EventEnvelope> = sku_001_events.iter()
        .chain(sku_002_events.iter())
        .chain(std::iter::once(&deploy_event))
        .cloned()
        .collect();

    // Project into default graph
    for ev in &all_events {
        projector.project(ev).await.expect("project event");
    }
    let default_before = count_default_graph_triples(&projector).await;
    assert!(default_before > 0);

    // --- Phase B: Pre-populate target graph with stale data ---
    let target_graph = ib.product_graph(product).to_string();
    projector
        .insert_triple_in_graph(
            "https://picloud.local/stale-aggregate-triple",
            &format!("{PICLOUD_NS}staleData"),
            oxigraph::model::Literal::new_simple_literal("old-sku-data").into(),
            &target_graph,
        )
        .expect("insert stale triple");
    let stale_count = count_triples_in_graph(&projector, &target_graph).await;
    assert!(stale_count > 0, "Target graph should have stale triples");

    // --- Phase C: Execute single-aggregate replay for sku-001 ---
    let replay_id = Uuid::new_v4();
    let request = ReplayRequest {
        from: now - chrono::Duration::hours(1),
        to: Some(now + chrono::Duration::hours(1)),
        product: Some(product.to_string()),
        aggregate_type: Some("SKU".to_string()),
        aggregate_ids: vec!["sku-001".to_string()],
    };

    let result = projector
        .execute_aggregate_replay(replay_id, &all_events, &request, Some(&target_graph))
        .expect("aggregate replay should succeed");

    // --- Invariant (a): Only sku-001 events were replayed ---
    assert_eq!(
        result.events_replayed, 3,
        "Only the 3 sku-001 events should be replayed (got {})",
        result.events_replayed
    );

    // --- Invariant (b): Shadow graph IRI follows convention ---
    assert!(
        result.shadow_graph_iri.starts_with("https://picloud.local/replay/"),
        "Shadow graph IRI should follow replay/{{id}} convention"
    );

    // --- Invariant (c): Target graph has replayed triples ---
    let target_after = count_triples_in_graph(&projector, &target_graph).await;
    assert!(
        target_after > 0,
        "Target graph should have triples from replayed sku-001 events"
    );

    // Stale data should be gone
    let stale_check = format!(
        r#"ASK WHERE {{ GRAPH <{target_graph}> {{ <https://picloud.local/stale-aggregate-triple> <{PICLOUD_NS}staleData> "old-sku-data" }} }}"#
    );
    let stale_result = projector.query(&stale_check).await.expect("stale check query");
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

    // --- Invariant (d): Shadow graph is empty after swap ---
    let shadow_residual = count_triples_in_graph(&projector, &result.shadow_graph_iri).await;
    assert_eq!(
        shadow_residual, 0,
        "Shadow graph must have zero triples after swap"
    );

    // --- Invariant (e): Default graph unchanged ---
    let default_after = count_default_graph_triples(&projector).await;
    assert_eq!(
        default_before, default_after,
        "Default (live) graph must not be modified by aggregate replay"
    );

    // --- Invariant (f): start_replay records aggregate scope metadata ---
    let start_request = ReplayRequest {
        from: now - chrono::Duration::hours(1),
        to: Some(now + chrono::Duration::hours(1)),
        product: Some(product.to_string()),
        aggregate_type: Some("SKU".to_string()),
        aggregate_ids: vec!["sku-001".to_string()],
    };
    let new_replay_id = projector
        .start_replay(start_request)
        .await
        .expect("start_replay should return replay_id");

    // Verify shadow graph has aggregate type metadata
    let shadow_iri = ib.replay_graph(&new_replay_id.to_string());
    let agg_type_sparql = format!(
        r#"SELECT ?aggType WHERE {{
            GRAPH <{}> {{
                ?s <{PICLOUD_NS}replayAggregateType> ?aggType
            }}
        }}"#,
        shadow_iri.as_str()
    );
    let agg_type_result = projector.query(&agg_type_sparql).await.expect("aggregate type query");
    assert!(
        !agg_type_result.bindings.is_empty(),
        "start_replay should record replayAggregateType metadata"
    );
    let agg_type_value = agg_type_result
        .bindings
        .first()
        .and_then(|b| b.get("aggType"))
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(
        agg_type_value, "SKU",
        "replayAggregateType should be 'SKU'"
    );

    // Verify shadow graph has aggregate ID metadata
    let agg_id_sparql = format!(
        r#"SELECT ?aggId WHERE {{
            GRAPH <{}> {{
                ?s <{PICLOUD_NS}replayAggregateId> ?aggId
            }}
        }}"#,
        shadow_iri.as_str()
    );
    let agg_id_result = projector.query(&agg_id_sparql).await.expect("aggregate id query");
    assert!(
        !agg_id_result.bindings.is_empty(),
        "start_replay should record replayAggregateId metadata"
    );
    let agg_id_value = agg_id_result
        .bindings
        .first()
        .and_then(|b| b.get("aggId"))
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(
        agg_id_value, "sku-001",
        "replayAggregateId should be 'sku-001'"
    );

    // Verify ReplayOperation type is present
    let type_sparql = format!(
        "SELECT ?type WHERE {{ GRAPH <{}> {{ ?s a ?type }} }}",
        shadow_iri.as_str()
    );
    let type_result = projector.query(&type_sparql).await.expect("type query");
    assert!(
        !type_result.bindings.is_empty(),
        "start_replay should create shadow graph with ReplayOperation metadata"
    );
}
