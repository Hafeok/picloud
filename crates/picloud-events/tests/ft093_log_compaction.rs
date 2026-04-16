/// FT-093 Integration Tests — Event log compaction and snapshotting
///
/// Covers TC-289, TC-346.
/// Tests that compaction reduces log size while preserving snapshot metadata,
/// and that the compacted log survives restarts with correct logical offsets.

use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::ResourceIri;
use picloud_domain::traits::EventLog;
use picloud_events::PersistentEventLog;
use uuid::Uuid;

/// Helper to build a test event with sensible defaults.
fn make_event(event_type: &str, product: Option<&str>, correlation_id: Uuid) -> EventEnvelope {
    EventEnvelope {
        id: Uuid::new_v4(),
        schema: ResourceIri::new(format!(
            "https://picloud.local/schemas/events/{event_type}/v1"
        ))
        .expect("valid schema IRI"),
        event_type: event_type.to_string(),
        timestamp: chrono::Utc::now(),
        source: ResourceIri::new("https://picloud.local/test").expect("valid source IRI"),
        product: product.map(|s| s.to_string()),
        correlation_id,
        idempotency_key: None,
        traceparent: None,
        replay: None,
        payload: serde_json::json!({}),
    }
}

// ============================================================================
// TC-289 — Event log compaction reduces log size while preserving snapshots
// ============================================================================
/// Scenario: Append many events to a persistent log, compact it, then verify
/// that (a) the on-disk file is smaller, (b) retained events are accessible
/// via their original logical offsets, and (c) snapshot_offset survives a
/// restart (re-open).
#[tokio::test]
async fn tc289_event_log_compaction_reduces_log_size_while_preserving_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let meta_path = dir.path().join("events.jsonl.meta");
    let cid = Uuid::new_v4();

    // --- Phase 1: Populate the log ---
    let total_events = 200;
    let discard_count = 150;
    let expected_remaining = total_events - discard_count; // 50

    let log = PersistentEventLog::open(&path).unwrap();

    for i in 0..total_events {
        log.append(make_event(&format!("Event{i}"), Some("test-product"), cid))
            .await
            .unwrap();
    }

    assert_eq!(log.len().await, total_events);
    assert_eq!(log.snapshot_offset().await, 0);

    let size_before = std::fs::metadata(&path).unwrap().len();

    // --- Phase 2: Compact the log ---
    log.compact(discard_count).await.unwrap();

    // Verify file size is reduced.
    let size_after = std::fs::metadata(&path).unwrap().len();
    assert!(
        size_after < size_before,
        "File should be smaller after compaction: {size_after} >= {size_before}"
    );

    // Verify in-memory event count.
    assert_eq!(log.len().await, expected_remaining);

    // Verify snapshot_offset tracks discarded events.
    assert_eq!(log.snapshot_offset().await, discard_count);

    // Verify total logical count is unchanged.
    assert_eq!(log.total_event_count().await, total_events);

    // Verify on-disk file has exactly `expected_remaining` lines.
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(content.lines().count(), expected_remaining);

    // --- Phase 3: Verify logical offsets work correctly ---

    // events_since(0) should return all remaining (offset below snapshot → returns all).
    let all_remaining = log.events_since(0).await;
    assert_eq!(all_remaining.len(), expected_remaining);
    assert_eq!(all_remaining[0].event_type, format!("Event{discard_count}"));

    // events_since at the snapshot boundary should return all remaining.
    let from_snapshot = log.events_since(discard_count).await;
    assert_eq!(from_snapshot.len(), expected_remaining);
    assert_eq!(
        from_snapshot[0].event_type,
        format!("Event{discard_count}")
    );

    // events_since at a logical offset within the retained range.
    let mid_offset = discard_count + 25;
    let from_mid = log.events_since(mid_offset).await;
    assert_eq!(from_mid.len(), 25);
    assert_eq!(from_mid[0].event_type, format!("Event{mid_offset}"));

    // events_since at the very end should return empty.
    let from_end = log.events_since(total_events).await;
    assert!(from_end.is_empty());

    // --- Phase 4: Verify snapshot metadata persists across restart ---

    // The sidecar meta file should exist.
    assert!(meta_path.exists(), "Sidecar .jsonl.meta file should exist");

    // Read the meta file to verify the snapshot offset is recorded.
    let meta_content = std::fs::read_to_string(&meta_path).unwrap();
    assert_eq!(
        meta_content.trim().parse::<usize>().unwrap(),
        discard_count
    );

    // Drop the log and re-open it.
    drop(log);

    let reopened = PersistentEventLog::open(&path).unwrap();

    // Verify state is restored correctly.
    assert_eq!(reopened.len().await, expected_remaining);
    assert_eq!(reopened.snapshot_offset().await, discard_count);
    assert_eq!(reopened.total_event_count().await, total_events);

    // Verify events are still readable via logical offsets after restart.
    let events_after_restart = reopened.events_since(discard_count).await;
    assert_eq!(events_after_restart.len(), expected_remaining);
    assert_eq!(
        events_after_restart[0].event_type,
        format!("Event{discard_count}")
    );
    assert_eq!(
        events_after_restart[expected_remaining - 1].event_type,
        format!("Event{}", total_events - 1)
    );
}

// ============================================================================
// TC-346 — Log compaction exit — log size reduced, snapshots preserved
// ============================================================================
/// Exit criteria: After a full compaction cycle including post-compaction
/// appends and a restart, verify all invariants hold: file size reduction,
/// correct event counts, snapshot_offset persistence, new-event append
/// capability, and correct logical offset resolution.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tc346_log_compaction_exit_log_size_reduced_snapshots_preserved() {
    eprintln!("TC346: START");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let meta_path = dir.path().join("events.jsonl.meta");
    let cid = Uuid::new_v4();

    // --- Setup: populate and compact ---
    let initial_count: usize = 100;
    let discard_count: usize = 70;
    let retained_count = initial_count - discard_count; // 30
    let post_compact_appends: usize = 15;

    eprintln!("TC346: opening log");
    let log = PersistentEventLog::open(&path).unwrap();

    eprintln!("TC346: appending {initial_count} events");
    for i in 0..initial_count {
        log.append(make_event(&format!("Event{i}"), Some("analytics"), cid))
            .await
            .unwrap();
    }
    eprintln!("TC346: done appending");

    let size_before = std::fs::metadata(&path).unwrap().len();
    eprintln!("TC346: size_before={size_before}");

    eprintln!("TC346: calling compact({discard_count}), len={}", log.len().await);
    log.compact(discard_count).await.unwrap();
    eprintln!("TC346: compact done");

    // --- EXIT CRITERION 1: Log size is reduced ---
    eprintln!("TC346: checking file size");
    let size_after_compact = std::fs::metadata(&path).unwrap().len();
    eprintln!("TC346: size_after={size_after_compact}");
    assert!(
        size_after_compact < size_before,
        "EXIT: log file must be smaller after compaction ({size_after_compact} >= {size_before})"
    );
    eprintln!("TC346: size assertion passed");

    // --- EXIT CRITERION 2: Remaining events match retention count ---
    eprintln!("TC346: checking len");
    let len_val = log.len().await;
    eprintln!("TC346: len={len_val}");
    assert_eq!(
        len_val,
        retained_count,
        "EXIT: in-memory event count must equal retained count"
    );
    eprintln!("TC346: len assertion passed");

    eprintln!("TC346: reading file lines");
    let file_lines = std::fs::read_to_string(&path).unwrap().lines().count();
    eprintln!("TC346: file_lines={file_lines}");
    assert_eq!(
        file_lines, retained_count,
        "EXIT: on-disk line count must equal retained count"
    );

    // --- EXIT CRITERION 3: Snapshot offset persisted in sidecar ---
    eprintln!("TC346: checking meta file");
    assert!(
        meta_path.exists(),
        "EXIT: sidecar .jsonl.meta file must exist"
    );
    let persisted_offset: usize = std::fs::read_to_string(&meta_path)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    eprintln!("TC346: persisted_offset={persisted_offset}");
    assert_eq!(
        persisted_offset, discard_count,
        "EXIT: persisted snapshot_offset must match discard count"
    );
    eprintln!("TC346: checking snapshot_offset");
    assert_eq!(
        log.snapshot_offset().await,
        discard_count,
        "EXIT: in-memory snapshot_offset must match discard count"
    );

    // --- EXIT CRITERION 4: New events can be appended post-compaction ---
    eprintln!("TC346: appending {post_compact_appends} post-compact events");
    for i in 0..post_compact_appends {
        eprintln!("TC346: appending PostCompact{i}");
        log.append(make_event(
            &format!("PostCompact{i}"),
            Some("analytics"),
            cid,
        ))
        .await
        .unwrap();
        eprintln!("TC346: appended PostCompact{i}");
    }

    eprintln!("TC346: post-compact appends done, checking total_event_count");
    let expected_total = initial_count + post_compact_appends;
    let total = log.total_event_count().await;
    eprintln!("TC346: total_event_count={total}");
    assert_eq!(
        total,
        expected_total,
        "EXIT: total logical event count must include pre- and post-compaction events"
    );

    eprintln!("TC346: checking len after post-compact appends");
    let len_after = log.len().await;
    eprintln!("TC346: len={len_after}");
    assert_eq!(
        len_after,
        retained_count + post_compact_appends,
        "EXIT: in-memory count must include retained + newly appended events"
    );

    // Verify logical offset resolution for post-compaction events.
    eprintln!("TC346: checking events_since({initial_count})");
    let post_events = log.events_since(initial_count).await;
    eprintln!("TC346: post_events.len={}", post_events.len());
    assert_eq!(
        post_events.len(),
        post_compact_appends,
        "EXIT: events_since(initial_count) must return only post-compaction events"
    );
    assert_eq!(post_events[0].event_type, "PostCompact0");
    eprintln!("TC346: post_events assertion passed");

    // Verify the full retained range is accessible.
    eprintln!("TC346: checking events_since({discard_count})");
    let all_events = log.events_since(discard_count).await;
    eprintln!("TC346: all_events.len={}", all_events.len());
    assert_eq!(
        all_events.len(),
        retained_count + post_compact_appends,
        "EXIT: events_since(snapshot_offset) must return all retained + new events"
    );
    assert_eq!(
        all_events[0].event_type,
        format!("Event{discard_count}")
    );
    assert_eq!(
        all_events.last().unwrap().event_type,
        format!("PostCompact{}", post_compact_appends - 1)
    );
    eprintln!("TC346: all range assertions passed");

    // --- EXIT CRITERION 5: All state survives a restart ---
    eprintln!("TC346: dropping log");
    drop(log);
    eprintln!("TC346: log dropped, checking file before reopen");
    let content = std::fs::read_to_string(&path).unwrap();
    eprintln!("TC346: file has {} lines", content.lines().count());
    drop(content);

    eprintln!("TC346: reopening via spawn_blocking");
    let path_clone = path.clone();
    let reopened = tokio::task::spawn_blocking(move || {
        PersistentEventLog::open(&path_clone).unwrap()
    }).await.unwrap();
    eprintln!("TC346: reopened");

    assert_eq!(
        reopened.snapshot_offset().await,
        discard_count,
        "EXIT (restart): snapshot_offset must survive restart"
    );
    assert_eq!(
        reopened.len().await,
        retained_count + post_compact_appends,
        "EXIT (restart): in-memory count must survive restart"
    );
    assert_eq!(
        reopened.total_event_count().await,
        expected_total,
        "EXIT (restart): total logical count must survive restart"
    );

    // Verify events readable after restart.
    let events_post_restart = reopened.events_since(discard_count).await;
    assert_eq!(events_post_restart.len(), retained_count + post_compact_appends);
    assert_eq!(events_post_restart[0].event_type, format!("Event{discard_count}"));
    assert_eq!(
        events_post_restart.last().unwrap().event_type,
        format!("PostCompact{}", post_compact_appends - 1)
    );

    // Verify a new event can be appended after restart.
    reopened
        .append(make_event("PostRestart0", Some("analytics"), cid))
        .await
        .unwrap();
    assert_eq!(
        reopened.total_event_count().await,
        expected_total + 1,
        "EXIT (restart): must be able to append after restart"
    );
}
