/// FT-068 — Freshness monitor — tracks maxAge per data product, emits
///          DataProductStale when breached.
///
/// Covers:
///   TC-273: Freshness monitor emits DataProductStale when maxAge exceeded (scenario)
///   TC-330: Freshness exit — DataProductStale emitted when maxAge breached (exit-criteria)
///
/// These tests verify that:
///   1. The RdfDataProductSLOMonitor detects when a data product's age exceeds
///      its declared maxAge and emits a Breach action.
///   2. The monitor correctly de-duplicates — a second check while still in
///      breach produces no new action.
///   3. When the data product is refreshed and now within SLO, the monitor
///      emits a Restore action.
///   4. The full lifecycle (declare → refresh → age past maxAge → breach →
///      re-refresh → restore) works end-to-end through the RDF graph.

use std::sync::Arc;

use chrono::{Duration, Utc};
use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{DataProductSLOAction, DataProductSLOMonitor, StateProjector};
use picloud_http::RdfDataProductSLOMonitor;
use picloud_rdf::OxigraphProjector;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

/// Declare a data product **with** a freshness SLO (maxAge).
fn make_data_product_declared_with_max_age(
    product: &str,
    dp_name: &str,
    domain: &str,
    version: &str,
    max_age: &str,
) -> EventEnvelope {
    let ib = iri_builder();
    let dp_iri = ib.data_product_graph(product, dp_name);
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
            "max_age": max_age,
        }),
    )
}

/// Simulate a DataProductRefreshed event with a specific `refreshed_at` timestamp.
fn make_data_product_refreshed_at(
    product: &str,
    dp_name: &str,
    refreshed_at: &str,
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
            "triple_count": 10,
            "duration_ms": 5,
            "trigger_event": "TestTrigger",
            "refreshed_at": refreshed_at,
        }),
    )
}

// ============================================================================
// TC-273 — Freshness monitor emits DataProductStale when maxAge exceeded
// ============================================================================
/// Scenario test:
///   1. Declare a data product with maxAge = PT5M (5 minutes).
///   2. Project a DataProductRefreshed event with a lastRefreshed timestamp
///      10 minutes in the past (well beyond maxAge).
///   3. Run the freshness monitor — it should emit a Breach action.
///   4. Run it again — the second call should NOT emit another Breach
///      (de-duplication).
#[tokio::test]
async fn tc273_freshness_monitor_emits_data_product_stale_when_max_age_exceeded() {
    let projector = Arc::new(OxigraphProjector::new().unwrap());

    // Step 1: Declare data product with a 5-minute maxAge SLO
    let declared = make_data_product_declared_with_max_age(
        "analytics-app",
        "user-metrics",
        "analytics",
        "1.0.0",
        "PT5M",
    );
    projector.project(&declared).await.unwrap();

    // Verify maxAge is stored in the RDF graph
    let dp_iri = iri_builder()
        .data_product_graph("analytics-app", "user-metrics")
        .as_str()
        .trim_end_matches("/graph")
        .to_string();
    let ask_max_age = format!(
        "ASK {{ <{dp_iri}> <https://picloud.local/ontology#maxAge> \"PT5M\" }}"
    );
    let result = projector.query(&ask_max_age).await.unwrap();
    assert_eq!(
        result.bindings[0]["result"], true,
        "maxAge should be stored in the RDF graph"
    );

    // Step 2: Project a refresh that happened 10 minutes ago (stale)
    let stale_time = (Utc::now() - Duration::minutes(10)).to_rfc3339();
    let refreshed = make_data_product_refreshed_at(
        "analytics-app",
        "user-metrics",
        &stale_time,
    );
    projector.project(&refreshed).await.unwrap();

    // Step 3: Run the freshness monitor — should detect breach
    let monitor = RdfDataProductSLOMonitor::new(projector.clone() as Arc<dyn StateProjector>);
    let actions = monitor.check_freshness().await.unwrap();

    assert_eq!(actions.len(), 1, "monitor should emit exactly one action");
    match &actions[0] {
        DataProductSLOAction::Breach(payload) => {
            assert!(
                payload.data_product_iri.as_str().contains("user-metrics"),
                "breach should reference the correct data product"
            );
            assert_eq!(payload.max_age, "PT5M", "breach should carry the maxAge");
            assert!(
                payload.actual_age_seconds >= 600,
                "actual age should be at least 600 seconds (10 min), got {}",
                payload.actual_age_seconds
            );
        }
        DataProductSLOAction::Restore(_) => {
            panic!("expected Breach, got Restore");
        }
    }

    // Step 4: Second check — should NOT emit duplicate breach
    let actions2 = monitor.check_freshness().await.unwrap();
    assert!(
        actions2.is_empty(),
        "second check should NOT emit duplicate breach (de-duplication)"
    );
}

// ============================================================================
// TC-330 — Freshness exit — DataProductStale emitted when maxAge breached
// ============================================================================
/// Exit-criteria test (full lifecycle):
///   1. Declare data product with maxAge = PT2M.
///   2. Refresh it with a timestamp only 1 minute old — within SLO.
///   3. Monitor check → no actions (within SLO).
///   4. Simulate time passing: re-project with a 5-minute-old lastRefreshed.
///   5. Monitor check → Breach action emitted.
///   6. Refresh the data product with a fresh timestamp (now).
///   7. Monitor check → Restore action emitted.
///   8. Final check → no further actions (stable, within SLO).
#[tokio::test]
async fn tc330_freshness_exit_data_product_stale_emitted_when_max_age_breached() {
    let projector = Arc::new(OxigraphProjector::new().unwrap());

    // Step 1: Declare data product with 2-minute maxAge
    let declared = make_data_product_declared_with_max_age(
        "reporting-app",
        "daily-summary",
        "reporting",
        "2.0.0",
        "PT2M",
    );
    projector.project(&declared).await.unwrap();

    // Step 2: Refresh with a timestamp only 1 minute ago (within SLO)
    let fresh_time = (Utc::now() - Duration::seconds(60)).to_rfc3339();
    let refreshed_fresh = make_data_product_refreshed_at(
        "reporting-app",
        "daily-summary",
        &fresh_time,
    );
    projector.project(&refreshed_fresh).await.unwrap();

    // Step 3: Monitor check → should be fine (within SLO)
    let monitor = RdfDataProductSLOMonitor::new(projector.clone() as Arc<dyn StateProjector>);
    let actions = monitor.check_freshness().await.unwrap();
    assert!(
        actions.is_empty(),
        "data product is within SLO — no actions expected"
    );

    // Step 4: Simulate staleness — re-project with 5-minute-old lastRefreshed.
    // We need to update the lastRefreshed triple. The projector's
    // project_data_product_refreshed replaces lastRefreshed.
    let stale_time = (Utc::now() - Duration::minutes(5)).to_rfc3339();
    let refreshed_stale = make_data_product_refreshed_at(
        "reporting-app",
        "daily-summary",
        &stale_time,
    );
    projector.project(&refreshed_stale).await.unwrap();

    // Step 5: Monitor check → Breach
    let actions = monitor.check_freshness().await.unwrap();
    assert_eq!(actions.len(), 1, "one breach action expected");
    match &actions[0] {
        DataProductSLOAction::Breach(payload) => {
            assert!(
                payload.data_product_iri.as_str().contains("daily-summary"),
                "breach should reference daily-summary"
            );
            assert_eq!(payload.max_age, "PT2M");
            assert!(
                payload.actual_age_seconds >= 300,
                "actual age should be ≥300s (5 min), got {}",
                payload.actual_age_seconds
            );
        }
        DataProductSLOAction::Restore(_) => panic!("expected Breach, got Restore"),
    }

    // Step 6: Refresh with a fresh timestamp (now — within SLO)
    let now_time = Utc::now().to_rfc3339();
    let refreshed_now = make_data_product_refreshed_at(
        "reporting-app",
        "daily-summary",
        &now_time,
    );
    projector.project(&refreshed_now).await.unwrap();

    // Step 7: Monitor check → Restore
    let actions = monitor.check_freshness().await.unwrap();
    assert_eq!(actions.len(), 1, "one restore action expected");
    match &actions[0] {
        DataProductSLOAction::Restore(payload) => {
            assert!(
                payload.data_product_iri.as_str().contains("daily-summary"),
                "restore should reference daily-summary"
            );
        }
        DataProductSLOAction::Breach(_) => panic!("expected Restore, got Breach"),
    }

    // Step 8: Final check → no actions (stable state)
    let actions = monitor.check_freshness().await.unwrap();
    assert!(
        actions.is_empty(),
        "after restore, no further actions expected"
    );
}
