/// TC-349 Regression Test — Event log replay does not hang on startup.
///
/// Guards against the startup hang observed in release builds on the Pi 5
/// cluster (node3, 2026-04-17) where `picloud-server` logs
/// `Cluster membership started`, then spins at 100% CPU indefinitely with
/// a non-empty `/var/lib/picloud/events` directory.
///
/// The root cause was `PersistentEventLog::open` calling
/// `futures::executor::block_on(...)` once per replayed event from inside
/// a synchronous constructor that is itself called from within a tokio
/// runtime context (`picloud-server` is `#[tokio::main]`). Nesting
/// `block_on` inside a tokio runtime is unsound — the larger the event
/// log, the more likely startup deadlocks or enters a futile poll loop.
///
/// The fix accesses the inner `RwLock` fields via `get_mut()` during
/// replay, which is sound because `inner` is exclusively owned at
/// construction time.
///
/// **Invariant under test:** `PersistentEventLog::open` must complete in
/// bounded wall-clock time for any well-formed event log of N entries
/// (N up to at least 10_000). It must not spin, infinite-loop, or hang on
/// replay, regardless of whether it is invoked inside a tokio runtime.
use std::time::{Duration, Instant};

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::ResourceIri;
use picloud_domain::traits::EventLog;
use picloud_events::PersistentEventLog;
use uuid::Uuid;

/// Upper bound on replay wall-clock time for each N. The observed hang
/// was infinite; 2s is the deadline specified by TC-349.
const REPLAY_DEADLINE: Duration = Duration::from_secs(2);

/// Build a synthetic event envelope of the given event type.
fn synthetic_event(event_type: &str, seq: usize) -> EventEnvelope {
    EventEnvelope {
        id: Uuid::new_v4(),
        schema: ResourceIri::new(format!(
            "https://picloud.local/schemas/events/{event_type}/v1"
        ))
        .expect("valid schema IRI"),
        event_type: event_type.to_string(),
        timestamp: chrono::Utc::now(),
        source: ResourceIri::new(format!("https://picloud.local/nodes/node{}", seq % 3 + 1))
            .expect("valid source IRI"),
        product: if seq % 4 == 0 {
            Some("photo-app".to_string())
        } else {
            None
        },
        correlation_id: Uuid::new_v4(),
        idempotency_key: Some(format!("tc349-key-{seq}")),
        traceparent: None,
        replay: None,
        payload: serde_json::json!({
            "seq": seq,
            "event_type": event_type,
        }),
    }
}

/// Cycle through the event types mentioned in TC-349.
fn event_type_for(seq: usize) -> &'static str {
    match seq % 3 {
        0 => "NodeJoined",
        1 => "LeaderElected",
        _ => "MetricRecorded",
    }
}

/// Pre-populate an event log with N synthetic events by writing the
/// NDJSON file directly. This simulates the disk state that would exist
/// on a node restart and is the most direct reproduction of the hang.
fn prepopulate_log(path: &std::path::Path, n: usize) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    let mut file = std::fs::File::create(path).expect("create event log file");
    use std::io::Write;
    for i in 0..n {
        let event = synthetic_event(event_type_for(i), i);
        let line = serde_json::to_string(&event).expect("serialize event");
        writeln!(file, "{}", line).expect("write event line");
    }
    file.flush().expect("flush event log");
}

/// Open a PersistentEventLog with N pre-existing events and assert that
/// `open()` returns within REPLAY_DEADLINE.
///
/// This test runs **inside a tokio runtime** on purpose: the original
/// hang only reproduced when `open` was called from within a tokio
/// runtime context (because the internal `futures::executor::block_on`
/// calls nested a second executor inside the first). Calling `open`
/// here from `#[tokio::test]` exercises the exact call-path that
/// `picloud-server` uses at startup.
async fn assert_open_bounded(n: usize) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("events.jsonl");

    prepopulate_log(&path, n);

    // The blocking work happens on the current tokio worker thread,
    // which is exactly how picloud-server invokes it during startup.
    let start = Instant::now();
    let log = PersistentEventLog::open(&path).expect("open should succeed");
    let elapsed = start.elapsed();

    assert!(
        elapsed < REPLAY_DEADLINE,
        "N={n}: PersistentEventLog::open took {elapsed:?}, must be < {REPLAY_DEADLINE:?} — \
         regression of the TC-349 startup hang"
    );

    assert_eq!(
        log.len().await,
        n,
        "N={n}: all pre-existing events must be loaded into memory"
    );

    // Verify events are readable via the logical offset interface,
    // so replay isn't merely 'fast but incorrect'.
    let all = log.events_since(0).await;
    assert_eq!(all.len(), n, "N={n}: events_since(0) must return all events");

    // Verify idempotency key deduplication survived replay — appending
    // an already-seen event must not double-store it. This catches a
    // replay that dropped events silently.
    if n > 0 {
        let mut dup = synthetic_event(event_type_for(0), 0);
        // force exact key collision with seq=0 (assigned above)
        dup.idempotency_key = Some("tc349-key-0".to_string());
        log.append(dup).await.expect("append should succeed");
        assert_eq!(
            log.len().await,
            n,
            "N={n}: duplicate idempotency key must be deduplicated after replay"
        );
    }
}

/// Empty-log open — must complete instantly with no events.
#[tokio::test]
async fn tc349_event_log_replay_does_not_hang_on_startup_n_0() {
    assert_open_bounded(0).await;
}

/// Small log — the smallest non-trivial case.
#[tokio::test]
async fn tc349_event_log_replay_does_not_hang_on_startup_n_100() {
    assert_open_bounded(100).await;
}

/// Medium log — roughly the size of the log observed on node3 (~387).
#[tokio::test]
async fn tc349_event_log_replay_does_not_hang_on_startup_n_1000() {
    assert_open_bounded(1000).await;
}

/// Large log — exercises the invariant for up to 10k events as specified.
#[tokio::test]
async fn tc349_event_log_replay_does_not_hang_on_startup_n_10000() {
    assert_open_bounded(10_000).await;
}

/// Canonical test function matching the runner-args name. Exercises the
/// full N sweep in one tokio runtime so the runner reports a single
/// outcome for TC-349.
#[tokio::test]
async fn tc349_event_log_replay_does_not_hang_on_startup() {
    for n in [0usize, 100, 1000, 10_000] {
        assert_open_bounded(n).await;
    }
}

/// Extra guard: open must also work on a single-threaded tokio runtime,
/// which is a common configuration that made the original deadlock
/// easier to reproduce (only one worker thread to park).
#[test]
fn tc349_event_log_replay_single_threaded_runtime() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build runtime");

    rt.block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("events.jsonl");

        // 500 events — well above the ~387 that triggered the hang.
        prepopulate_log(&path, 500);

        let start = Instant::now();
        let log = PersistentEventLog::open(&path).expect("open should not hang");
        let elapsed = start.elapsed();

        assert!(
            elapsed < REPLAY_DEADLINE,
            "single-threaded replay took {elapsed:?}, must be < {REPLAY_DEADLINE:?}"
        );

        assert_eq!(log.len().await, 500);
    });
}
