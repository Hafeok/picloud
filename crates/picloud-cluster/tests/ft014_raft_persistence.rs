//! FT-014: Raft consensus and leader election — persistence tests.
//!
//! TC-225: Cluster survives one node restart without data loss.
//! Validates that a persistent Raft node (sled-backed) retains committed
//! entries and state machine progress across shutdown and restart.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use openraft::BasicNode;
use picloud_cluster::{create_persistent_raft_node, ClientRequest, PiCloudRaft};

/// Wait for this node to become leader within `timeout`.
async fn wait_for_leader(raft: &PiCloudRaft, node_id: u64, timeout: std::time::Duration) {
    let mut rx = raft.metrics();
    tokio::time::timeout(timeout, async {
        loop {
            let m = rx.borrow_and_update().clone();
            if m.current_leader == Some(node_id) {
                break;
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .expect("node did not become leader within timeout");
}

/// TC-225: Verify that a persistent single-node Raft cluster retains all
/// committed entries after a full shutdown and restart from the same sled
/// database directory.
///
/// Steps:
/// 1. Create a persistent Raft node (sled-backed) and bootstrap as single-node cluster.
/// 2. Write several entries through Raft consensus.
/// 3. Verify apply callback fires for each entry.
/// 4. Shut down the Raft node cleanly.
/// 5. Re-open the sled database and create a new Raft node from the same path.
/// 6. Verify the state machine starts with the correct applied count
///    (entries survived the restart).
/// 7. Write additional entries and verify indexing continues from the
///    persisted offset (no data loss, no double-apply).
#[tokio::test]
async fn tc225_cluster_survives_one_node_restart_without_data_loss() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = tmp.path().join("raft_db");
    let node_id: u64 = 1;
    let addr = "127.0.0.1:0"; // not actually listening in this test

    let apply_count = Arc::new(AtomicU64::new(0));

    // ---------------------------------------------------------------
    // Phase 1: Bootstrap, write entries, shut down
    // ---------------------------------------------------------------
    let entries_written: u64 = 5;
    {
        let counter = Arc::clone(&apply_count);
        let callback: picloud_cluster::ApplyCallback =
            Arc::new(move |_json: &str| {
                counter.fetch_add(1, Ordering::SeqCst);
            });

        let mut members = BTreeMap::new();
        members.insert(node_id, BasicNode::new(addr));

        let raft = create_persistent_raft_node(
            node_id,
            addr,
            Some(callback),
            Some(members),
            &db_path,
        )
        .await
        .expect("failed to create persistent raft node");

        wait_for_leader(&raft, node_id, std::time::Duration::from_secs(5)).await;

        // Write entries
        for i in 1..=entries_written {
            let resp = raft
                .client_write(ClientRequest {
                    event_json: format!(r#"{{"seq": {i}, "data": "event-{i}"}}"#),
                })
                .await
                .expect("client_write failed");

            assert_eq!(resp.data.index, i, "applied index should match sequence");
        }

        // Verify all entries applied via callback
        assert_eq!(
            apply_count.load(Ordering::SeqCst),
            entries_written,
            "apply callback should have fired for each entry"
        );

        // Clean shutdown
        raft.shutdown().await.expect("shutdown failed");
    }

    // ---------------------------------------------------------------
    // Phase 2: Restart from same sled directory — verify data survived
    // ---------------------------------------------------------------
    let restart_apply_count = Arc::new(AtomicU64::new(0));
    {
        let counter = Arc::clone(&restart_apply_count);
        let callback: picloud_cluster::ApplyCallback =
            Arc::new(move |_json: &str| {
                counter.fetch_add(1, Ordering::SeqCst);
            });

        // Re-open from same path — no members (already initialized)
        let raft = create_persistent_raft_node(
            node_id,
            addr,
            Some(callback),
            None, // do NOT re-initialize — state is persisted
            &db_path,
        )
        .await
        .expect("failed to restart persistent raft node");

        wait_for_leader(&raft, node_id, std::time::Duration::from_secs(5)).await;

        // The state machine should NOT have re-applied old entries on restart
        // (sled persists last_applied_log, so openraft skips already-applied entries).
        // The restart callback count should be 0 — no re-application.
        assert_eq!(
            restart_apply_count.load(Ordering::SeqCst),
            0,
            "restart should not re-apply already-committed entries"
        );

        // Verify we can write new entries that continue from the persisted offset
        let new_entries: u64 = 3;
        for i in 1..=new_entries {
            let resp = raft
                .client_write(ClientRequest {
                    event_json: format!(r#"{{"seq": {}, "data": "post-restart-{i}"}}"#, entries_written + i),
                })
                .await
                .expect("client_write after restart failed");

            // Index should continue from where we left off
            assert_eq!(
                resp.data.index,
                entries_written + i,
                "post-restart index should continue from persisted state"
            );
        }

        // The callback should have fired only for the new entries
        assert_eq!(
            restart_apply_count.load(Ordering::SeqCst),
            new_entries,
            "only new entries should trigger the callback after restart"
        );

        raft.shutdown().await.expect("shutdown after restart failed");
    }

    // ---------------------------------------------------------------
    // Phase 3: Verify sled database directly — log entries persisted
    // ---------------------------------------------------------------
    {
        let db = sled::open(&db_path).expect("failed to open sled for verification");
        let log_tree = db.open_tree("raft_log").expect("raft_log tree missing");

        // Should have log entries persisted (may be compacted, but at minimum
        // the entries from phase 2 should be present).
        assert!(
            !log_tree.is_empty(),
            "raft_log tree should contain persisted entries"
        );

        // Verify state machine metadata survived
        let sm_tree = db.open_tree("sm_meta").expect("sm_meta tree missing");
        assert!(
            sm_tree.contains_key(b"sm_last_applied").expect("sled read error"),
            "sm_last_applied should be persisted"
        );
        assert!(
            sm_tree.contains_key(b"sm_count").expect("sled read error"),
            "sm_count should be persisted"
        );

        // Verify the persisted count matches total entries written
        let count_bytes = sm_tree
            .get(b"sm_count")
            .expect("sled read error")
            .expect("sm_count missing");
        let count: u64 =
            serde_json::from_slice(&count_bytes).expect("failed to deserialize sm_count");
        assert_eq!(
            count,
            entries_written + 3,
            "persisted sm_count should equal total entries written across both phases"
        );

        drop(db);
    }
}
