/// TC-357 Regression Test — Event store read with `limit=N` must surface
/// a just-appended event, even when the underlying log already contains
/// many more than `limit` events for that Product.
///
/// Guards against the failure observed on the Pi 5 cluster (2026-04-18)
/// where the `event-store-append-read` E2E scenario reported
/// `event evt-<uuid> not found in event store read response` after a
/// 2011ms duration. The previous TC-351 test passed in isolation because
/// every iteration started with an empty `InMemoryEventLog` — so the
/// newly-appended event was the only event for the product and naturally
/// fit inside any reasonable `limit`. On the live cluster the log already
/// held hundreds of historical product events, and the E2E scenario uses
/// `limit=10`, so the freshly-appended event fell past the truncation
/// boundary and was never returned.
///
/// The root cause was in `handle_event_store_api_read` —
/// `all_events.into_iter().filter(..).take(limit)` yields the **oldest**
/// N events rather than the newest, which is the opposite of what any
/// event-log consumer expects.
///
/// **Invariant under test:** with a persistent event log that already
/// contains more than `limit` events for a Product, appending a new event
/// and reading `GET /api/event-store/:product/events?limit=N` MUST return
/// the new event within the slice. Because the fixed read handler returns
/// events most-recent-first, the just-appended event must also appear at
/// index 0.
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder};
use picloud_domain::traits::EventLog;
use picloud_events::InMemoryEventLog;
use picloud_http::PiCloudHttpServer;
use tower::util::ServiceExt;
use uuid::Uuid;

/// Pre-seed the event log with this many product events so the store is
/// already "fat" relative to any reasonable `limit` query parameter.
/// Mirrors the live-cluster condition where prior E2E runs left a log
/// containing many more events than the scenario's `limit=10`.
const SEED_COUNT: usize = 200;

/// Upper bound on the append-to-read round trip. Matches TC-351 so that
/// any regression in the append/read hot path also fails this test.
const READ_DEADLINE: Duration = Duration::from_secs(5);
const INITIAL_POLL_DELAY: Duration = Duration::from_millis(1);
const MAX_POLL_DELAY: Duration = Duration::from_millis(200);

/// Build a router backed by an in-memory event log pre-seeded with
/// `seed` product events spread across multiple aggregate IDs.
async fn build_router_with_seed(
    product: &str,
    seed: usize,
) -> (axum::Router, Arc<InMemoryEventLog>) {
    let cluster = ClusterDomain::default();
    let log = Arc::new(InMemoryEventLog::new());
    let iri_builder = IriBuilder::new(cluster.clone());

    for i in 0..seed {
        let aggregate_id = format!("photo-{}", i % 17); // spread across aggregates
        let event_id = format!("seed-evt-{}", Uuid::new_v4());
        let payload = serde_json::json!({
            "type": "PhotoCreated",
            "aggregateType": "Photo",
            "aggregateId": aggregate_id,
            "schema": "https://picloud.local/schemas/events/PhotoCreated/v1",
            "payload": {
                "eventId": event_id,
                "title": format!("Seed #{i}"),
                "size": 1024u64 + i as u64,
            }
        });

        let envelope = EventEnvelope::new(
            iri_builder.event_schema("PhotoCreated", 1),
            "PhotoCreated",
            iri_builder.resource(product, "event-store", "main"),
            Some(product.to_string()),
            Uuid::new_v4(),
            payload,
        );

        log.append(envelope)
            .await
            .expect("seed append must succeed");
    }

    let mut server = PiCloudHttpServer::new("127.0.0.1:0".parse().unwrap(), cluster);
    server.event_log = Some(log.clone() as Arc<dyn EventLog>);
    (server.build_router(), log)
}

/// Append an event via the HTTP API and return the assigned event id.
async fn append_event(
    app: &axum::Router,
    product: &str,
    body: &serde_json::Value,
) -> Result<StatusCode, String> {
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/event-store/{}/append", product))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .map_err(|e| format!("build append request: {e}"))?;

    let resp = app
        .clone()
        .oneshot(req)
        .await
        .map_err(|e| format!("append oneshot: {e}"))?;

    Ok(resp.status())
}

/// Poll `GET /api/event-store/:product/events?limit=N` until an event
/// whose nested `payload.eventId` matches `event_id` appears, or until
/// `READ_DEADLINE` elapses. Returns the full JSON response and the index
/// at which the matching event was found.
async fn poll_for_event(
    app: &axum::Router,
    product: &str,
    event_id: &str,
    limit: usize,
) -> Result<(Duration, serde_json::Value, usize), String> {
    let start = Instant::now();
    let mut delay = INITIAL_POLL_DELAY;

    loop {
        let read_path = format!("/api/event-store/{}/events?limit={}", product, limit);
        let req = Request::builder()
            .method(Method::GET)
            .uri(&read_path)
            .body(Body::empty())
            .map_err(|e| format!("build read request: {e}"))?;

        let resp = app
            .clone()
            .oneshot(req)
            .await
            .map_err(|e| format!("read oneshot: {e}"))?;

        if resp.status() != StatusCode::OK {
            return Err(format!(
                "event store read returned status {} (expected 200)",
                resp.status()
            ));
        }

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .map_err(|e| format!("read body: {e}"))?;
        let json: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|e| format!("parse read body: {e}"))?;

        if let Some(events) = json["events"].as_array() {
            for (idx, event) in events.iter().enumerate() {
                if event["payload"]["payload"]["eventId"].as_str() == Some(event_id) {
                    return Ok((start.elapsed(), event.clone(), idx));
                }
            }
        }

        if start.elapsed() >= READ_DEADLINE {
            return Err(format!(
                "event {event_id} not found in event store read response after {:?} (limit={})",
                READ_DEADLINE, limit
            ));
        }

        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(MAX_POLL_DELAY);
    }
}

/// Core assertion: with a pre-seeded log, appending a new event and
/// reading with `limit=N` must surface the new event.
async fn append_and_read_returns_new_event(limit: usize) {
    let product = "picloud-test";

    // Fill the log with significantly more than `limit` events for the
    // Product. `SEED_COUNT` is intentionally much larger than any limit
    // the test uses, so a naive `take(limit)` at the head of the
    // chronological iterator would never reach the just-appended event.
    let (app, log) = build_router_with_seed(product, SEED_COUNT).await;
    assert!(
        log.len().await >= SEED_COUNT,
        "precondition: log should contain at least {SEED_COUNT} seed events"
    );

    let event_id = format!("evt-{}", Uuid::new_v4());
    let body = serde_json::json!({
        "type": "PhotoCreated",
        "aggregateType": "Photo",
        "aggregateId": "photo-new",
        "schema": "https://picloud.local/schemas/events/PhotoCreated/v1",
        "payload": {
            "eventId": event_id,
            "title": "Fresh upload",
            "size": 9999u64,
        }
    });

    let status = append_event(&app, product, &body)
        .await
        .expect("append request must build and dispatch");
    assert!(
        status.is_success(),
        "append must succeed (got {status}) — limit={limit}"
    );

    let (elapsed, event, index) = poll_for_event(&app, product, &event_id, limit)
        .await
        .unwrap_or_else(|e| panic!("TC-357 regression (limit={limit}): {e}"));

    assert!(
        elapsed < READ_DEADLINE,
        "append→read latency {:?} exceeds deadline {:?} (limit={limit})",
        elapsed,
        READ_DEADLINE,
    );

    // Payload integrity — the fresh event's nested eventId round-trips.
    assert_eq!(
        event["payload"]["payload"]["eventId"].as_str(),
        Some(event_id.as_str()),
        "limit={limit}: returned event must carry the freshly appended eventId",
    );

    // Most-recent-first semantics: the just-appended event must appear
    // at index 0 of the read response.
    assert_eq!(
        index, 0,
        "limit={limit}: just-appended event must appear first \
         (most-recent-first semantics) but was at index {index}",
    );

    // Envelope-level fields survive the round trip.
    assert_eq!(
        event["event_type"].as_str(),
        Some("PhotoCreated"),
        "limit={limit}: event_type must be preserved"
    );
    assert_eq!(
        event["payload"]["schema"].as_str(),
        Some("https://picloud.local/schemas/events/PhotoCreated/v1"),
        "limit={limit}: schema IRI must be preserved"
    );
}

/// Canonical test function matching the runner-args name. Exercises
/// `limit=10` (matching the live E2E scenario that failed on the Pi
/// cluster) and `limit=1` (to make the boundary failure explicit —
/// a single append followed by a one-element read MUST yield the
/// just-appended event).
#[tokio::test]
async fn tc357_event_store_read_limit_returns_latest() {
    // limit=10 reproduces the exact configuration that failed on the Pi
    // cluster with `event evt-<uuid> not found`.
    append_and_read_returns_new_event(10).await;

    // limit=1 is the strictest boundary — a single-element read must
    // return the just-appended event, not the first event ever recorded.
    append_and_read_returns_new_event(1).await;

    // limit=100 matches TC-351's request to confirm the fix did not
    // regress the wider-window case that was already passing.
    append_and_read_returns_new_event(100).await;
}
