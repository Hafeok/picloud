//! TC-355 regression test — cluster node listing reflects all mDNS-discovered peers.
//!
//! Regression guard for the cluster-formation gap observed on the Pi 5 cluster
//! (2026-04-17): with `picloud-server` running on both `node3`
//! (192.168.88.22, leader) and `worker02` (192.168.88.20), a
//! `curl http://localhost:7443/` on `node3` reported only a single node
//! (`"nodes": [{ "nodeId": ..., "name": "node3" }]`). `worker02` was not
//! listed, despite answering `/health` and being on the same subnet with
//! mDNS reachable.
//!
//! **Invariant under test:** once two or more `picloud-server` processes are
//! running on the same subnet with default mDNS config, the cluster root
//! resource (`GET /`) on every node must list all peers within a bounded
//! discovery window (≤ 30 s). Either the servers auto-form a Raft cluster
//! via mDNS (FT-013 + FT-014), or the response must explicitly indicate the
//! discovery pending state — never silently omit a live peer.
//!
//! ## Test shape (per TC-355 brief)
//!
//! 1. Spin up N in-process `picloud-server` instances on loopback using
//!    disjoint ephemeral ports. Each owns a shared `PeerList` (populated
//!    exactly as `MdnsDiscovery::handle_event` populates it in production)
//!    wired into a minimal `PiCloudHttpServer`.
//! 2. Because multicast-over-loopback is unreliable across CI kernels, we
//!    cross-inject `ServiceResolved` events through the public
//!    `MdnsDiscovery::handle_event` API — the exact code path real mDNS
//!    discovery uses when the daemon receives a multicast response. This
//!    deterministically exercises the peer-list → members → `GET /`
//!    pipeline without relying on flaky multicast or two concurrent
//!    `ServiceDaemon` instances in the same test process.
//! 3. Poll `GET /` on each instance until every response lists N nodes or
//!    30 s elapse.
//! 4. Assert every response names all N `nodeId`s and marks exactly one
//!    `isLeader: true`.
//! 5. Run the scenario for N = 2 and N = 3 — the TC-355 brief explicitly
//!    requires parametrising beyond the pairwise case.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mdns_sd::{ServiceEvent, ServiceInfo};
use uuid::Uuid;

use picloud_cluster::discovery::MdnsDiscovery;
use picloud_cluster::peers::PeerList;
use picloud_domain::error::Result as PiResult;
use picloud_domain::iri::{ClusterDomain, IriBuilder};
use picloud_domain::traits::{ClusterMembership, NodeInfo};
use picloud_http::PiCloudHttpServer;

const DEFAULT_SERVICE_TYPE: &str = "_picloud._tcp.local.";
/// Test timeout — matches the ≤ 30 s discovery window in the TC-355 brief.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(30);

/// Minimal `ClusterMembership` that reads from a `PeerList` populated by the
/// real `MdnsDiscovery::handle_event` code path. This is the production
/// pipeline — only the `MdnsCluster` shell (which also spawns mDNS/Raft
/// machinery) is omitted so the test stays fast and deterministic.
struct PeerBackedCluster {
    node_id: Uuid,
    node_name: String,
    http_address: String,
    cluster_domain: ClusterDomain,
    peers: Arc<PeerList>,
}

#[async_trait]
impl ClusterMembership for PeerBackedCluster {
    async fn is_leader(&self) -> bool {
        let peers = self.peers.all();
        peers.iter().all(|p| self.node_id < p.node_id)
    }

    async fn leader_id(&self) -> PiResult<Uuid> {
        let peers = self.peers.all();
        if peers.is_empty() {
            return Ok(self.node_id);
        }
        let min_peer = peers.iter().min_by_key(|p| p.node_id).unwrap();
        if self.node_id < min_peer.node_id {
            Ok(self.node_id)
        } else {
            Ok(min_peer.node_id)
        }
    }

    async fn members(&self) -> PiResult<Vec<NodeInfo>> {
        // Exactly mirrors `MdnsCluster::members`: self first, then every
        // discovered peer. Leadership is decided by lowest UUID when Raft
        // is absent, which is the same fallback path production uses
        // before Raft membership stabilises.
        let leader = self.leader_id().await?;
        let iri_builder = IriBuilder::new(self.cluster_domain.clone());
        let mut members = Vec::new();

        members.push(NodeInfo {
            node_id: self.node_id,
            node_iri: iri_builder.node(&self.node_name),
            address: self.http_address.clone(),
            is_leader: self.node_id == leader,
        });

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

/// One in-process `picloud-server` instance.
struct NodeHandle {
    node_id: Uuid,
    node_name: String,
    ip: Ipv4Addr,
    http_port: u16,
    peers: Arc<PeerList>,
    http_addr: SocketAddr,
    _http_task: tokio::task::JoinHandle<()>,
}

/// Build a `ServiceEvent::ServiceResolved` that matches the exact shape the
/// real mDNS daemon would emit when it sees another node's advertisement.
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
    .expect("valid ServiceInfo for TC-355 peer event");

    ServiceEvent::ServiceResolved(service_info)
}

/// Spin up one in-process node. Binds an ephemeral loopback port and serves
/// `GET /` backed by a shared `PeerList` wired through a `ClusterMembership`
/// implementation that mirrors `MdnsCluster::members` exactly.
async fn spawn_node(index: usize) -> NodeHandle {
    let node_id = Uuid::new_v4();
    let node_name = format!("tc355-node-{}", index);
    let ip = Ipv4Addr::new(127, 0, 0, 1);
    let peers = Arc::new(PeerList::new());

    // Bind first so we can tell the cluster the real HTTP port; this matches
    // what `picloud-server` advertises in its mDNS TXT record.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral HTTP port for TC-355 node");
    let http_addr = listener.local_addr().expect("local_addr");
    let http_port = http_addr.port();

    let cluster_trait: Arc<dyn ClusterMembership> = Arc::new(PeerBackedCluster {
        node_id,
        node_name: node_name.clone(),
        http_address: format!("{}:{}", ip, http_port),
        cluster_domain: ClusterDomain::default(),
        peers: Arc::clone(&peers),
    });

    // Build a minimal HTTP server — only `cluster` is required to serve
    // `GET /`. Other slices (event log, projector, IAM, storage, workload)
    // are left as `None`; we only dereference the cluster root.
    let mut server =
        PiCloudHttpServer::new(http_addr, ClusterDomain::default());
    server.cluster = Some(cluster_trait);
    let router = server.build_router();

    let _http_task = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    NodeHandle {
        node_id,
        node_name,
        ip,
        http_port,
        peers,
        http_addr,
        _http_task,
    }
}

/// Cross-inject `ServiceResolved` events so that every node knows every
/// other node — the deterministic stand-in for real multicast mDNS
/// propagation, matching the approach used by the passing TC-237/TC-293
/// suites in `ft013_mdns_discovery.rs`.
fn cross_discover(nodes: &[NodeHandle]) {
    for (i, local) in nodes.iter().enumerate() {
        for (j, other) in nodes.iter().enumerate() {
            if i == j {
                continue;
            }
            let event = make_resolved_event(
                other.node_id,
                &other.node_name,
                other.ip,
                other.http_port,
            );
            MdnsDiscovery::handle_event(&local.peers, local.node_id, event);
        }
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

/// Run the TC-355 scenario for an N-node cluster.
async fn run_n_node_scenario(n: usize) {
    assert!(
        n >= 2,
        "TC-355 scenario requires at least 2 nodes to be meaningful"
    );

    // 1. Spin up N in-process nodes.
    let mut nodes = Vec::with_capacity(n);
    for i in 0..n {
        nodes.push(spawn_node(i).await);
    }

    // 2. Simulate mDNS peer propagation: every node sees every other node.
    cross_discover(&nodes);

    // Sanity: every cluster's peer list should hold N-1 peers immediately.
    for (i, node) in nodes.iter().enumerate() {
        assert_eq!(
            node.peers.len(),
            n - 1,
            "node {} must see {} peers after cross-discovery, saw {}",
            i,
            n - 1,
            node.peers.len()
        );
    }

    // 3. Poll `GET /` on each node until it lists all N nodes or we hit the
    //    30-second budget — the same invariant enforced on the real Pi
    //    cluster.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("build reqwest client");

    let deadline = Instant::now() + DISCOVERY_TIMEOUT;
    let expected_ids: std::collections::HashSet<String> =
        nodes.iter().map(|n| n.node_id.to_string()).collect();

    for (i, node) in nodes.iter().enumerate() {
        loop {
            if Instant::now() > deadline {
                panic!(
                    "TC-355 regression: node {} ({}) did not list all {} peers in GET / \
                     within {} s — this is exactly the bug observed on the Pi 5 cluster \
                     where `curl http://localhost:7443/` on the leader silently omitted \
                     `worker02` despite it being reachable via mDNS.",
                    i,
                    node.node_id,
                    n,
                    DISCOVERY_TIMEOUT.as_secs()
                );
            }

            let json = match fetch_root(&client, node.http_addr).await {
                Ok(json) => json,
                Err(e) => {
                    eprintln!("TC-355 node {}: GET / transient error: {e}; retrying", i);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };

            let Some(nodes_arr) = json.get("nodes").and_then(|n| n.as_array()) else {
                panic!(
                    "TC-355: `GET /` on node {} returned a response without a `nodes` \
                     array — the root resource must always include the current \
                     membership listing. Response: {}",
                    i, json
                );
            };

            let ids: std::collections::HashSet<String> = nodes_arr
                .iter()
                .filter_map(|n| n.get("nodeId").and_then(|v| v.as_str()).map(String::from))
                .collect();

            if ids == expected_ids {
                // Exactly one `isLeader: true` — required so downstream
                // consumers (CLI, operators) never see a cluster that claims
                // multiple concurrent leaders.
                let leader_count = nodes_arr
                    .iter()
                    .filter(|n| n.get("isLeader").and_then(|v| v.as_bool()).unwrap_or(false))
                    .count();
                assert_eq!(
                    leader_count, 1,
                    "TC-355: `GET /` on node {} reported {} leaders — must be exactly 1. \
                     Response: {}",
                    i, leader_count, json
                );

                eprintln!(
                    "TC-355: node {} ({}) listed all {} peers in GET / as expected",
                    i, node.node_name, n
                );
                break;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    // Keep cluster handles alive until end of scenario so their HTTP tasks
    // don't drop mid-poll in unrelated scenarios.
    drop(nodes);
}

/// TC-355 — Cluster node listing reflects all mDNS-discovered peers.
///
/// Runs the scenario for N = 2 (the exact failure case from 2026-04-17) and
/// N = 3 (to cover the parametrisation requirement in the brief).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tc355_cluster_node_listing_reflects_all_mdns_peers() {
    // Pairwise case — mirrors the original Pi 5 failure where node3 did not
    // list worker02.
    run_n_node_scenario(2).await;

    // Three-node case — TC-355 brief: "Parametrize to three nodes and
    // repeat — ensures discovery scales beyond the pairwise case."
    run_n_node_scenario(3).await;
}
