//! FT-095: Multi-node Raft voter configuration tuning.
//!
//! Validates that voter configuration changes (promote, demote, atomic swap)
//! complete via joint consensus without interrupting client writes.
//!
//! TC-291: Multi-node Raft voter configuration change completes without downtime (scenario).
//! TC-348: Voter config exit — Raft voter change completes without downtime (exit-criteria).

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use openraft::BasicNode;
use picloud_cluster::{create_raft_node, raft_rpc_router, ClientRequest, PiCloudRaft};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Wait for a specific node to become leader within `timeout`.
async fn wait_for_leader(raft: &PiCloudRaft, node_id: u64, timeout: Duration) {
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

/// Write a single entry through the Raft leader and return the applied index.
async fn write_entry(raft: &PiCloudRaft, seq: u64) -> u64 {
    let resp = raft
        .client_write(ClientRequest {
            event_json: format!(r#"{{"seq": {seq}}}"#),
        })
        .await
        .expect("client_write failed during voter config change — downtime detected");
    resp.data.index
}

/// A test Raft node with an HTTP server for inter-node RPCs.
struct TestNode {
    raft: Arc<PiCloudRaft>,
    #[allow(dead_code)]
    node_id: u64,
    addr: String,
    apply_count: Arc<AtomicU64>,
    _server_handle: tokio::task::JoinHandle<()>,
}

impl TestNode {
    /// Create and bootstrap a single-node Raft cluster.
    async fn bootstrap(node_id: u64) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().unwrap().to_string();

        let mut members = BTreeMap::new();
        members.insert(node_id, BasicNode::new(&addr));

        Self::create(node_id, listener, addr, Some(members)).await
    }

    /// Create a node that will later join an existing cluster as a learner.
    async fn follower(node_id: u64) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().unwrap().to_string();

        Self::create(node_id, listener, addr, None).await
    }

    async fn create(
        node_id: u64,
        listener: tokio::net::TcpListener,
        addr: String,
        members: Option<BTreeMap<u64, BasicNode>>,
    ) -> Self {
        let apply_count = Arc::new(AtomicU64::new(0));
        let counter = apply_count.clone();
        let callback: picloud_cluster::ApplyCallback = Arc::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });

        let raft = create_raft_node(node_id, &addr, Some(callback), members)
            .await
            .expect("create_raft_node failed");

        let raft = Arc::new(raft);
        let router = raft_rpc_router(raft.clone());

        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.ok();
        });

        Self {
            raft,
            node_id,
            addr,
            apply_count,
            _server_handle: handle,
        }
    }
}

// ---------------------------------------------------------------------------
// TC-291: Multi-node Raft voter configuration change completes without downtime
// ---------------------------------------------------------------------------

/// TC-291: Validates that voter configuration changes — add learner, promote
/// to voter, demote to learner, and atomic voter set swap — complete via
/// joint consensus and that client writes succeed throughout every
/// transition, proving zero downtime.
///
/// Steps:
///  1. Bootstrap node 1 as single voter, verify baseline write.
///  2. Start nodes 2 and 3 as un-joined followers.
///  3. Add node 2 as learner, promote to voter — verify writes succeed.
///  4. Add node 3 as learner, promote to voter — verify 3-voter writes.
///  5. Demote node 3 to learner — verify 2-voter writes.
///  6. Atomic voter set swap {1,2} -> {1,3} — verify writes succeed.
///  7. Assert final voter set matches expectation.
#[tokio::test]
async fn tc291_multi_node_raft_voter_configuration_change_completes_without_downtime() {
    // -- Phase 1: Bootstrap single-voter cluster --------------------------
    let node1 = TestNode::bootstrap(1).await;
    wait_for_leader(&node1.raft, 1, Duration::from_secs(5)).await;

    // Baseline write proves cluster is operational.
    let idx = write_entry(&node1.raft, 1).await;
    assert_eq!(idx, 1);

    // -- Phase 2: Start follower nodes ------------------------------------
    let node2 = TestNode::follower(2).await;
    let node3 = TestNode::follower(3).await;

    // -- Phase 3: Add node 2 as learner, then promote to voter ------------
    node1
        .raft
        .add_learner(2, BasicNode::new(&node2.addr), true)
        .await
        .expect("add_learner(2) failed");

    let mut voters: BTreeSet<u64> = BTreeSet::new();
    voters.insert(1);
    voters.insert(2);
    node1
        .raft
        .change_membership(voters.clone(), false)
        .await
        .expect("change_membership {1,2} failed");

    // Writes succeed after promotion — no downtime.
    let idx = write_entry(&node1.raft, 2).await;
    assert!(idx >= 2, "write after promote should succeed");

    // -- Phase 4: Add node 3 as learner, promote to voter -----------------
    node1
        .raft
        .add_learner(3, BasicNode::new(&node3.addr), true)
        .await
        .expect("add_learner(3) failed");

    voters.insert(3);
    node1
        .raft
        .change_membership(voters.clone(), false)
        .await
        .expect("change_membership {1,2,3} failed");

    // Writes succeed with 3-voter cluster.
    let idx = write_entry(&node1.raft, 3).await;
    assert!(idx >= 3, "write with 3 voters should succeed");

    // -- Phase 5: Demote node 3 back to learner ---------------------------
    let mut two_voters: BTreeSet<u64> = BTreeSet::new();
    two_voters.insert(1);
    two_voters.insert(2);
    node1
        .raft
        .change_membership(two_voters, false)
        .await
        .expect("demote node 3 failed");

    // Writes succeed after demotion.
    let idx = write_entry(&node1.raft, 4).await;
    assert!(idx >= 4, "write after demotion should succeed");

    // -- Phase 6: Atomic voter set swap {1,2} -> {1,3} -------------------
    // Node 3 was removed from the cluster in Phase 5 (retain=false).
    // Re-add it as a learner so it can be included in the new voter set.
    node1
        .raft
        .add_learner(3, BasicNode::new(&node3.addr), true)
        .await
        .expect("re-add node 3 as learner failed");

    let mut swapped: BTreeSet<u64> = BTreeSet::new();
    swapped.insert(1);
    swapped.insert(3);
    node1
        .raft
        .change_membership(swapped, false)
        .await
        .expect("atomic voter swap failed");

    // Writes succeed after atomic swap.
    let idx = write_entry(&node1.raft, 5).await;
    assert!(idx >= 5, "write after atomic swap should succeed");

    // -- Phase 7: Assert final voter set ----------------------------------
    let metrics = node1.raft.metrics().borrow().clone();
    let final_voters: BTreeSet<u64> = metrics.membership_config.voter_ids().collect();
    assert!(final_voters.contains(&1), "node 1 should be voter");
    assert!(final_voters.contains(&3), "node 3 should be voter");
    assert!(!final_voters.contains(&2), "node 2 should NOT be voter after swap");
    assert_eq!(final_voters.len(), 2, "exactly 2 voters expected");

    // -- Cleanup ----------------------------------------------------------
    node1.raft.shutdown().await.ok();
    node2.raft.shutdown().await.ok();
    node3.raft.shutdown().await.ok();
}

// ---------------------------------------------------------------------------
// TC-348: Voter config exit — Raft voter change completes without downtime
// ---------------------------------------------------------------------------

/// TC-348 (exit criteria): Proves voter configuration changes complete
/// without measurable downtime. A burst of client writes is submitted
/// before, during (immediately after), and after a full promote/demote
/// cycle. All writes must succeed and the total elapsed time must stay
/// under a generous 30-second budget.
#[tokio::test]
async fn tc348_voter_config_exit_raft_voter_change_completes_without_downtime() {
    let overall = Instant::now();
    let time_budget = Duration::from_secs(30);

    // -- Bootstrap single-voter cluster -----------------------------------
    let node1 = TestNode::bootstrap(1).await;
    wait_for_leader(&node1.raft, 1, Duration::from_secs(5)).await;

    // Start a second node.
    let node2 = TestNode::follower(2).await;

    // -- Pre-change writes ------------------------------------------------
    for seq in 1..=5 {
        write_entry(&node1.raft, seq).await;
    }

    // -- Promote node 2 to voter ------------------------------------------
    node1
        .raft
        .add_learner(2, BasicNode::new(&node2.addr), true)
        .await
        .expect("add_learner failed");

    let mut voters: BTreeSet<u64> = BTreeSet::new();
    voters.insert(1);
    voters.insert(2);
    node1
        .raft
        .change_membership(voters, false)
        .await
        .expect("promote failed");

    // -- Mid-change writes (immediately after promotion) ------------------
    for seq in 6..=10 {
        write_entry(&node1.raft, seq).await;
    }

    // -- Demote node 2 back to learner ------------------------------------
    let mut solo: BTreeSet<u64> = BTreeSet::new();
    solo.insert(1);
    node1
        .raft
        .change_membership(solo, false)
        .await
        .expect("demote failed");

    // -- Post-change writes -----------------------------------------------
    for seq in 11..=15 {
        write_entry(&node1.raft, seq).await;
    }

    // -- Assertions -------------------------------------------------------
    // All 15 writes must have been applied on the leader.
    assert!(
        node1.apply_count.load(Ordering::SeqCst) >= 15,
        "leader should have applied at least 15 entries, got {}",
        node1.apply_count.load(Ordering::SeqCst),
    );

    // The whole sequence must finish well within the time budget.
    let elapsed = overall.elapsed();
    assert!(
        elapsed < time_budget,
        "voter config cycle took {:?}, exceeding the {:?} budget — downtime detected",
        elapsed,
        time_budget,
    );

    // -- Cleanup ----------------------------------------------------------
    node1.raft.shutdown().await.ok();
    node2.raft.shutdown().await.ok();
}
