/// TC-351 Regression Test — Event store append then read returns the
/// appended event within bounded latency.
///
/// Guards against the failure observed on the Pi 5 cluster (2026-04-17)
/// where the `event-store-append-read` E2E scenario reported
/// `event evt-<uuid> not found in event store read response` after a
/// 2010ms duration. The prior scenario used a fixed 2-second sleep
/// followed by a single read — the 2010ms miss suggests either (a) the
/// read API was queried before the event was durably visible, or (b) the
/// Raft commit + projection path took longer than the hard-coded sleep.
///
/// **Invariant under test:** an event appended to a Product event store
/// via `POST /api/event-store/:product/append` must be observable through
/// the read API (`GET /api/event-store/:product/events`) within a bounded
/// wait of 5 seconds (longer than any plausible Raft commit + projection
/// lag on the target hardware). Both the payload body and the schema IRI
/// must be preserved in the read response.
///
/// This test codifies the documented upper bound rather than relying on
/// hope. If the HTTP append handler, the in-memory event log, or any
/// wrapper around them regresses such that a just-appended event cannot
/// be read within 5 seconds, this test will fail.
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use picloud_domain::iri::ClusterDomain;
use picloud_domain::traits::EventLog;
use picloud_events::InMemoryEventLog;
use picloud_http::PiCloudHttpServer;
use tower::util::ServiceExt;
use uuid::Uuid;

/// Documented upper bound on append-to-read visibility latency.
const READ_DEADLINE: Duration = Duration::from_secs(5);

/// Number of serial iterations. The TC specifies 50 to flush out
/// flakiness in the write-then-read path.
const ITERATIONS: usize = 50;

/// Initial backoff between polls. Doubles up to a cap to mirror a real
/// client's exponential-backoff read loop.
const INITIAL_POLL_DELAY: Duration = Duration::from_millis(1);
const MAX_POLL_DELAY: Duration = Duration::from_millis(200);

/// Build a router backed by an in-memory event log. This exercises the
/// HTTP handlers for `/api/event-store/:product/append` and
/// `/api/event-store/:product/events` against the same code path the
/// cluster uses, minus the Raft replication layer.
fn build_router() -> axum::Router {
    let mut server = PiCloudHttpServer::new(
        "127.0.0.1:0".parse().unwrap(),
        ClusterDomain::default(),
    );
    server.event_log = Some(Arc::new(InMemoryEventLog::new()) as Arc<dyn EventLog>);
    server.build_router()
}

/// Append an event via `POST /api/event-store/:product/append` and
/// return the append response status.
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

/// Poll `GET /api/event-store/:product/events` until an event with the
/// given `eventId` payload field appears, or until `READ_DEADLINE` elapses.
/// On success, return the elapsed wall-clock time and the matching event's
/// JSON representation.
async fn poll_for_event(
    app: &axum::Router,
    product: &str,
    event_id: &str,
) -> Result<(Duration, serde_json::Value), String> {
    let start = Instant::now();
    let mut delay = INITIAL_POLL_DELAY;

    loop {
        let read_path = format!("/api/event-store/{}/events?limit=100", product);
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
            for event in events {
                // The append handler stores the entire POST body as the
                // envelope payload, so the client-supplied `payload` key
                // is one level deeper in the read response.
                if event["payload"]["payload"]["eventId"].as_str() == Some(event_id) {
                    return Ok((start.elapsed(), event.clone()));
                }
            }
        }

        if start.elapsed() >= READ_DEADLINE {
            return Err(format!(
                "event {event_id} not found in event store read response after {:?}",
                READ_DEADLINE
            ));
        }

        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(MAX_POLL_DELAY);
    }
}

/// Execute a single append-then-read round trip against a freshly built
/// router. Returns the elapsed read-visibility time on success.
async fn single_round_trip(iteration: usize) -> Result<Duration, String> {
    let app = build_router();
    let product = "picloud-test";

    let event_id = format!("evt-{}", Uuid::new_v4());
    let schema_iri = "https://picloud.local/schemas/events/PhotoCreated/v1";
    let aggregate_id = format!("photo-{}", Uuid::new_v4());
    let title = format!("Test Photo #{iteration}");
    let size: u64 = 1024 + iteration as u64;

    let body = serde_json::json!({
        "type": "PhotoCreated",
        "aggregateType": "Photo",
        "aggregateId": aggregate_id,
        "schema": schema_iri,
        "payload": {
            "eventId": event_id,
            "title": title,
            "size": size,
        }
    });

    let status = append_event(&app, product, &body).await?;
    if !status.is_success() {
        return Err(format!(
            "iteration {iteration}: append returned status {status}"
        ));
    }

    let (elapsed, event) = poll_for_event(&app, product, &event_id).await?;

    // Assert bounded latency — strictly within READ_DEADLINE.
    if elapsed >= READ_DEADLINE {
        return Err(format!(
            "iteration {iteration}: event visible in {:?}, exceeds deadline {:?}",
            elapsed, READ_DEADLINE
        ));
    }

    // Assert payload integrity — the round-tripped payload must preserve
    // every field the client sent. The append handler stores the entire
    // POST body as the envelope payload, so the client's `payload` key
    // is nested one level deeper in the read response.
    let outer = &event["payload"];
    let inner = &outer["payload"];

    if inner["eventId"].as_str() != Some(event_id.as_str()) {
        return Err(format!(
            "iteration {iteration}: payload.eventId mismatch (got {:?}, want {event_id})",
            inner["eventId"].as_str()
        ));
    }
    if inner["title"].as_str() != Some(title.as_str()) {
        return Err(format!(
            "iteration {iteration}: payload.title mismatch (got {:?}, want {title})",
            inner["title"].as_str()
        ));
    }
    if inner["size"].as_u64() != Some(size) {
        return Err(format!(
            "iteration {iteration}: payload.size mismatch (got {:?}, want {size})",
            inner["size"].as_u64()
        ));
    }

    // Assert schema IRI preservation — the client-supplied schema IRI
    // must be retrievable from the read response (carried through the
    // outer payload because the append body becomes the stored payload).
    if outer["schema"].as_str() != Some(schema_iri) {
        return Err(format!(
            "iteration {iteration}: payload.schema mismatch (got {:?}, want {schema_iri})",
            outer["schema"].as_str()
        ));
    }

    // Assert event_type preservation at the envelope level.
    if event["event_type"].as_str() != Some("PhotoCreated") {
        return Err(format!(
            "iteration {iteration}: event_type mismatch (got {:?}, want PhotoCreated)",
            event["event_type"].as_str()
        ));
    }

    Ok(elapsed)
}

/// Canonical test function matching the runner-args name. Exercises the
/// full 50-iteration sweep serially so the runner reports a single
/// outcome for TC-351.
#[tokio::test]
async fn tc351_event_store_append_then_read_returns_event() {
    let overall_start = Instant::now();
    let mut latencies: Vec<Duration> = Vec::with_capacity(ITERATIONS);

    for i in 0..ITERATIONS {
        match single_round_trip(i).await {
            Ok(elapsed) => latencies.push(elapsed),
            Err(msg) => panic!("TC-351 regression: {msg}"),
        }
    }

    let total = overall_start.elapsed();
    let max = latencies.iter().copied().max().unwrap_or_default();
    let sum: Duration = latencies.iter().copied().sum();
    let avg = sum / (latencies.len().max(1) as u32);

    // Final invariant: the slowest single round trip must still sit
    // strictly below READ_DEADLINE. Each per-iteration assertion already
    // enforces this, but surfacing the aggregate makes regressions
    // easier to triage when the test eventually fails.
    assert!(
        max < READ_DEADLINE,
        "TC-351: max append→read latency {:?} exceeds deadline {:?} (avg {:?}, total {:?})",
        max,
        READ_DEADLINE,
        avg,
        total,
    );
}

/// A smaller single-iteration variant that can be run quickly for
/// sanity-checking during development. Uses the exact same flow as the
/// 50-iteration canonical test.
#[tokio::test]
async fn tc351_event_store_append_then_read_single() {
    single_round_trip(0)
        .await
        .expect("single append-then-read round trip must succeed within deadline");
}
