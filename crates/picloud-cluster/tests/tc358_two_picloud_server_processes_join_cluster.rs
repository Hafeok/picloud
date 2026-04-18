//! TC-358 regression test — two picloud-server processes on separate Pi
//! nodes join a single Raft cluster via mDNS.
//!
//! Observed symptom (2026-04-18, Pi 5 cluster): `picloud-server` on
//! `node3` (192.168.88.22) and `worker02` (192.168.88.20) each came up
//! healthy but reported *different* `cluster_id` values with only
//! themselves in `nodes[]`. After 10+ seconds neither had discovered the
//! other via mDNS → Raft enrollment; the existing `MdnsDiscovery` unit
//! tests (TC-237 / TC-293) both pass on localhost, so the regression is
//! specifically in the handoff between discovery and Raft enrollment
//! when running as real binaries on the LAN.
//!
//! ## Invariant under test
//!
//! When two `picloud-server` processes start on the same LAN with the
//! same cluster domain, the second to start MUST join the first's
//! cluster. Specifically, within 30 s of the second node's start:
//!
//! - Both nodes' `GET /` reports the same `cluster_id`.
//! - Both nodes' `GET /` lists both nodes in `nodes[]`.
//! - Exactly one node has `isLeader: true`.
//!
//! ## Shape of the test
//!
//! Per the TC-358 brief, we bind two in-process `picloud-server`
//! instances on distinct loopback ports. Each instance wires:
//!
//! 1. A `PeerList` that mDNS discovery populates in production. We
//!    cross-inject `ServiceResolved` events through
//!    `MdnsDiscovery::handle_event` — the identical code path used by
//!    the real mDNS daemon, and the same deterministic approach TC-237
//!    / TC-293 / TC-355 use to sidestep flaky multicast-over-loopback.
//! 2. A `PiCloudRaft` node — the first to start bootstraps as a
//!    single-node Raft cluster (matching the lowest-UUID tiebreaker
//!    decision in `src/main.rs`); the second starts *without* initial
//!    members and waits to be added as a learner by the leader.
//! 3. A learner-promotion loop that polls the `PeerList` (same shape as
//!    the one in `src/main.rs`, with a tighter 100 ms interval so the
//!    test converges in seconds rather than minutes) and calls
//!    `add_learner` + `change_membership` on newly discovered peers.
//! 4. A minimal `PiCloudHttpServer` wired to the cluster membership and
//!    Raft RPC router, so each node exposes `GET /`, `GET /health`, and
//!    the `/raft/*` endpoints the peers need for consensus RPCs.
//!
//! The test then polls `GET /` on both nodes until the invariant above
//! holds, matching the exact shape described in the TC-358 brief.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mdns_sd::{ServiceEvent, ServiceInfo};
use openraft::BasicNode;
use tokio::sync::RwLock;
use uuid::Uuid;

use picloud_cluster::discovery::MdnsDiscovery;
use picloud_cluster::peers::PeerList;
use picloud_cluster::{create_raft_node, node_id_from_uuid, raft_rpc_router, PiCloudRaft};
use picloud_domain::error::Result as PiResult;
use picloud_domain::iri::{ClusterDomain, IriBuilder};
use picloud_domain::traits::{ClusterMembership, NodeInfo};
use picloud_http::PiCloudHttpServer;

const DEFAULT_SERVICE_TYPE: &str = "_picloud._tcp.local.";
/// End-to-end convergence budget — matches the 30 s invariant in the
/// TC-358 brief.
const CLUSTER_FORMATION_TIMEOUT: Duration = Duration::from_secs(30);
/// How long we let a fresh Raft node elect its first leader. Single-node
/// bootstrap typically completes in under `election_timeout_max` (3 s);
/// 10 s gives us a generous cushion on slow CI.
const LEADER_ELECTION_TIMEOUT: Duration = Duration::from_secs(10);

/// Build a `ServiceEvent::ServiceResolved` matching the exact shape that
/// the real mDNS daemon emits when it resolves another node's advert.
fn make_resolved_event(
    node_id: Uuid,
    node_name: &str,
    ip: Ipv4Addr,
    http_port: u16,
) -> ServiceEvent {
    let mut properties = HashMap::new();
    properties.insert("node_id".to_string(), node_id.to_string());
    properties.insert("http_port".to_string(), http_port.to_string());

    let host = format!("{}.local.", node_name);
    let service_info = ServiceInfo::new(
        DEFAULT_SERVICE_TYPE,
        node_name,
        &host,
        IpAddr::V4(ip),
        http_port,
        properties,
    )
    .expect("valid ServiceInfo for TC-358 peer event");

    ServiceEvent::ServiceResolved(service_info)
}

/// `ClusterMembership` impl backed by a live Raft handle + a `PeerList`.
///
/// Shape mirrors `MdnsCluster::is_leader` / `leader_id` / `members` from
/// production: when Raft has elected a leader, that leader's node ID is
/// authoritative; otherwise each node falls back to reporting itself as
/// leader. The HTTP root handler's `cluster_id` derives from
/// `leader_id().await`, so once Raft converges both nodes surface the
/// same `cluster_id`.
struct TestCluster {
    node_id: Uuid,
    node_name: String,
    http_address: String,
    cluster_domain: ClusterDomain,
    peers: Arc<PeerList>,
    raft: Arc<PiCloudRaft>,
}

#[async_trait]
impl ClusterMembership for TestCluster {
    async fn is_leader(&self) -> bool {
        let me = node_id_from_uuid(&self.node_id);
        self.raft.current_leader().await == Some(me)
    }

    async fn leader_id(&self) -> PiResult<Uuid> {
        let me = node_id_from_uuid(&self.node_id);
        if let Some(leader_raft) = self.raft.current_leader().await {
            if leader_raft == me {
                return Ok(self.node_id);
            }
            for peer in self.peers.all() {
                if node_id_from_uuid(&peer.node_id) == leader_raft {
                    return Ok(peer.node_id);
                }
            }
        }
        // Pre-convergence fallback — each node reports itself. The test
        // polls until both nodes agree, so the transient mismatch is
        // expected and harmless.
        Ok(self.node_id)
    }

    async fn members(&self) -> PiResult<Vec<NodeInfo>> {
        let leader = self.leader_id().await?;
        let iri_builder = IriBuilder::new(self.cluster_domain.clone());
        let mut members = vec![NodeInfo {
            node_id: self.node_id,
            node_iri: iri_builder.node(&self.node_name),
            address: self.http_address.clone(),
            is_leader: self.node_id == leader,
        }];
        for peer in self.peers.all() {
            members.push(NodeInfo {
                node_id: peer.node_id,
                node_iri: iri_builder.node(&peer.node_name),
                address: peer.address.clone(),
                is_leader: peer.node_id == leader,
            });
        }
        Ok(members)
    }

    async fn local_node_id(&self) -> Uuid {
        self.node_id
    }
}

/// One in-process picloud-server-equivalent instance.
struct NodeHandle {
    node_id: Uuid,
    node_name: String,
    ip: Ipv4Addr,
    http_port: u16,
    http_addr: SocketAddr,
    peers: Arc<PeerList>,
    _raft: Arc<PiCloudRaft>,
    _server_task: tokio::task::JoinHandle<()>,
    _learner_task: tokio::task::JoinHandle<()>,
}

/// Bootstrap mode — `true` for the node that starts the single-node
/// Raft cluster (the lowest-UUID node in the fixed-UUID test scenario,
/// matching the tiebreaker in `src/main.rs`).
async fn start_node(node_id: Uuid, node_name: String, bootstrap: bool) -> NodeHandle {
    let ip = Ipv4Addr::new(127, 0, 0, 1);

    // Bind the HTTP port first so we can tell Raft the exact raft_addr
    // peers will use (same approach as `src/main.rs`, where `raft_addr`
    // derives from `bind_addr` + `http_port`).
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind TC-358 HTTP port");
    let http_addr = listener.local_addr().expect("local_addr");
    let http_port = http_addr.port();
    let raft_addr = format!("{}:{}", ip, http_port);
    let raft_node_id = node_id_from_uuid(&node_id);

    let peers = Arc::new(PeerList::new());

    // Bootstrap path: the lowest-UUID node starts the cluster with just
    // itself; other nodes start *without* initial members and will be
    // added as learners by the leader (matches the fix in `src/main.rs`).
    let initial_members = if bootstrap {
        let mut members = BTreeMap::new();
        members.insert(raft_node_id, BasicNode::new(&raft_addr));
        Some(members)
    } else {
        None
    };

    let raft = Arc::new(
        create_raft_node(raft_node_id, &raft_addr, None, initial_members)
            .await
            .expect("create_raft_node"),
    );

    let cluster_trait: Arc<dyn ClusterMembership> = Arc::new(TestCluster {
        node_id,
        node_name: node_name.clone(),
        http_address: raft_addr.clone(),
        cluster_domain: ClusterDomain::default(),
        peers: Arc::clone(&peers),
        raft: Arc::clone(&raft),
    });

    // Build the HTTP server with the cluster trait + the Raft RPC router
    // (needed so peers can reach `/raft/vote`, `/raft/append`, and
    // `/raft/snapshot`).
    let raft_router = raft_rpc_router(Arc::clone(&raft));
    let mut server = PiCloudHttpServer::new(http_addr, ClusterDomain::default());
    server.cluster = Some(cluster_trait);
    server.extra_router = Some(raft_router);
    let router = server.build_router();

    let _server_task = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    // Learner-promotion loop — same shape as the 5 s loop in
    // `src/main.rs`, tightened to 100 ms so the test converges quickly.
    // Only the current Raft leader adds learners / changes voter sets.
    let known_members: Arc<RwLock<HashSet<u64>>> = {
        let mut set = HashSet::new();
        set.insert(raft_node_id);
        Arc::new(RwLock::new(set))
    };
    let raft_for_loop = Arc::clone(&raft);
    let peers_for_loop = Arc::clone(&peers);
    let _learner_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let metrics = raft_for_loop.metrics().borrow().clone();
            if metrics.current_leader != Some(raft_node_id) {
                continue;
            }
            let current_peers = peers_for_loop.all();
            for peer in &current_peers {
                let peer_raft_id = node_id_from_uuid(&peer.node_id);
                if known_members.read().await.contains(&peer_raft_id) {
                    continue;
                }
                let peer_node = BasicNode::new(&peer.address);
                match raft_for_loop.add_learner(peer_raft_id, peer_node, true).await {
                    Ok(_) => {
                        known_members.write().await.insert(peer_raft_id);
                        let members: BTreeSet<u64> =
                            known_members.read().await.iter().copied().collect();
                        if let Err(e) = raft_for_loop.change_membership(members, false).await {
                            eprintln!(
                                "TC-358 [{}]: change_membership failed: {e}",
                                raft_node_id
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "TC-358 [{}]: add_learner {peer_raft_id} failed: {e}",
                            raft_node_id
                        );
                    }
                }
            }
        }
    });

    NodeHandle {
        node_id,
        node_name,
        ip,
        http_port,
        http_addr,
        peers,
        _raft: raft,
        _server_task,
        _learner_task,
    }
}

async fn fetch_root(
    client: &reqwest::Client,
    addr: SocketAddr,
) -> reqwest::Result<serde_json::Value> {
    let url = format!("http://{}/", addr);
    let resp = client.get(&url).send().await?;
    resp.json::<serde_json::Value>().await
}

async fn wait_for_health(
    client: &reqwest::Client,
    addr: SocketAddr,
    timeout: Duration,
    label: &str,
) {
    let deadline = Instant::now() + timeout;
    let url = format!("http://{}/health", addr);
    loop {
        if Instant::now() > deadline {
            panic!("TC-358: /health on {label} ({addr}) not ok within {timeout:?}");
        }
        match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => return,
            _ => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
}

/// Block until this node's Raft reports itself as leader (or timeout).
/// Required before we call `add_learner` from the learner loop — a
/// single-node bootstrap typically elects itself within `election_timeout_max`
/// (3 s by default); this gives us a 10 s cushion on slow CI.
async fn wait_for_self_leader(raft: &PiCloudRaft, node_id: u64, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() > deadline {
            panic!("TC-358: Raft node {node_id} did not elect itself leader within {timeout:?}");
        }
        if raft.current_leader().await == Some(node_id) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// TC-358 — Two picloud-server processes on separate Pi nodes join a
/// single Raft cluster via mDNS.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tc358_two_picloud_server_processes_join_single_cluster_via_mdns() {
    // Fixed UUIDs so the lowest-UUID tiebreaker is deterministic across
    // runs — this matches the `src/main.rs` tiebreaker that decides
    // which of two racing picloud-server processes actually bootstraps.
    let id_a = Uuid::parse_str("00000000-0000-0000-0000-000000000001")
        .expect("parse id_a");
    let id_b = Uuid::parse_str("ffffffff-ffff-ffff-ffff-fffffffffff0")
        .expect("parse id_b");
    assert!(id_a < id_b, "id_a must be the lowest UUID");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("build reqwest client");

    // Step 1 — start node A first; as lowest-UUID it bootstraps a
    // single-node Raft cluster.
    let node_a = start_node(id_a, "tc358-node-a".to_string(), true).await;
    wait_for_health(
        &client,
        node_a.http_addr,
        Duration::from_secs(10),
        "node-a",
    )
    .await;
    // Give A's single-node cluster a moment to elect itself leader
    // before B joins — otherwise A's learner loop can't add B.
    wait_for_self_leader(&node_a._raft, node_id_from_uuid(&id_a), LEADER_ELECTION_TIMEOUT).await;

    // Step 2 — staggered 500 ms boot (per TC-358 shape), then node B,
    // which is *not* a bootstrap node (higher UUID) and waits for A to
    // enroll it as a learner.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let node_b = start_node(id_b, "tc358-node-b".to_string(), false).await;
    wait_for_health(
        &client,
        node_b.http_addr,
        Duration::from_secs(10),
        "node-b",
    )
    .await;

    // Step 3 — cross-inject mDNS discovery events (same public code path
    // real mDNS uses). This simulates the multicast propagation that
    // happens on the real LAN once both nodes are advertising.
    MdnsDiscovery::handle_event(
        &node_a.peers,
        id_a,
        make_resolved_event(id_b, &node_b.node_name, node_b.ip, node_b.http_port),
    );
    MdnsDiscovery::handle_event(
        &node_b.peers,
        id_b,
        make_resolved_event(id_a, &node_a.node_name, node_a.ip, node_a.http_port),
    );

    // Step 4 — poll `GET /` on both nodes until the TC-358 invariant
    // holds (≤ 30 s).
    let expected_ids: HashSet<String> = [id_a.to_string(), id_b.to_string()]
        .into_iter()
        .collect();
    let deadline = Instant::now() + CLUSTER_FORMATION_TIMEOUT;
    loop {
        if Instant::now() > deadline {
            // Grab one last snapshot so the failure message is useful.
            let a = fetch_root(&client, node_a.http_addr).await.ok();
            let b = fetch_root(&client, node_b.http_addr).await.ok();
            panic!(
                "TC-358 regression: cluster did not form within {}s.\n  \
                 node-a GET /: {:?}\n  node-b GET /: {:?}\n\n\
                 This is exactly the bug observed on the Pi 5 cluster (2026-04-18) \
                 where `picloud-server` on node3 and worker02 each came up with \
                 their own `cluster_id` and never merged. Either the discovery → \
                 Raft enrollment handshake broke again, or the HTTP root handler's \
                 `cluster_id` is no longer derived from the Raft leader.",
                CLUSTER_FORMATION_TIMEOUT.as_secs(),
                a,
                b,
            );
        }

        let (Ok(json_a), Ok(json_b)) = (
            fetch_root(&client, node_a.http_addr).await,
            fetch_root(&client, node_b.http_addr).await,
        ) else {
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        };

        let cid_a = json_a
            .get("cluster_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let cid_b = json_b
            .get("cluster_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let nodes_a = json_a
            .get("nodes")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let nodes_b = json_b
            .get("nodes")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let ids_a: HashSet<String> = nodes_a
            .iter()
            .filter_map(|n| n.get("nodeId").and_then(|v| v.as_str()).map(String::from))
            .collect();
        let ids_b: HashSet<String> = nodes_b
            .iter()
            .filter_map(|n| n.get("nodeId").and_then(|v| v.as_str()).map(String::from))
            .collect();

        if cid_a.is_empty() || cid_a != cid_b || ids_a != expected_ids || ids_b != expected_ids {
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        }

        // The three TC-358 invariants on the invariant list in the brief:
        //   1. cluster_id matches across both responses ✓ (above)
        //   2. Both nodes appear in `nodes[]` on every node        ✓ (above)
        //   3. Exactly one `isLeader: true` across the two responses.
        let leaders_a = nodes_a
            .iter()
            .filter(|n| {
                n.get("isLeader")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            })
            .count();
        let leaders_b = nodes_b
            .iter()
            .filter(|n| {
                n.get("isLeader")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(
            leaders_a, 1,
            "TC-358: node-a `GET /` reported {leaders_a} leaders — must be exactly 1. \
             Response: {json_a}"
        );
        assert_eq!(
            leaders_b, 1,
            "TC-358: node-b `GET /` reported {leaders_b} leaders — must be exactly 1. \
             Response: {json_b}"
        );

        eprintln!(
            "TC-358: cluster formed with cluster_id = {cid_a} (elapsed: {:?})",
            deadline.saturating_duration_since(Instant::now())
        );
        break;
    }

    // Keep node handles alive until the assertions complete so the
    // background tasks don't tear down mid-poll.
    drop((node_a, node_b));
}
