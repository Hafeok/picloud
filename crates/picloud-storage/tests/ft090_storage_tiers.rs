/// FT-090 Integration Tests — Additional storage intent tiers
///
/// Covers TC-286, TC-343.
/// Tests that the storage backend correctly maps durability tiers (quorum,
/// local, archive, fast) to replication targets, focusing on quorum tier
/// replicating to a majority of cluster nodes.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

use picloud_domain::error::Result;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::resources::VolumeType;
use picloud_domain::storage::{DurabilityTier, PerformanceTier, StorageIntent};
use picloud_domain::traits::{ClusterMembership, NodeInfo, StorageBackend};
use picloud_storage::LocalStorageBackend;

fn iri_builder() -> IriBuilder {
    IriBuilder::new(ClusterDomain::default())
}

fn make_volume_iri(name: &str) -> ResourceIri {
    iri_builder().resource("test-product", "volumes", name)
}

fn temp_base_path() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("picloud-ft090-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

/// A mock ClusterMembership that returns a configurable set of nodes.
struct MockCluster {
    local_id: Uuid,
    nodes: Vec<Uuid>,
}

impl MockCluster {
    fn new(local_id: Uuid, nodes: Vec<Uuid>) -> Self {
        Self { local_id, nodes }
    }
}

#[async_trait]
impl ClusterMembership for MockCluster {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn leader_id(&self) -> Result<Uuid> {
        Ok(self.local_id)
    }
    async fn members(&self) -> Result<Vec<NodeInfo>> {
        let builder = IriBuilder::new(ClusterDomain::default());
        Ok(self
            .nodes
            .iter()
            .map(|id| NodeInfo {
                node_id: *id,
                node_iri: builder.resource("cluster", "nodes", &id.to_string()),
                address: format!("10.0.0.{}", id.as_bytes()[0]),
                is_leader: *id == self.local_id,
            })
            .collect())
    }
    async fn local_node_id(&self) -> Uuid {
        self.local_id
    }
}

/// Helper: create a storage intent with the given durability and performance tiers.
fn intent(durability: DurabilityTier, performance: PerformanceTier) -> StorageIntent {
    StorageIntent {
        durability,
        performance,
        snapshots: None,
        offsite: None,
    }
}

/// Helper: create a backend with the given cluster size (returns local_id and all node ids).
fn make_cluster_backend(
    base: &PathBuf,
    cluster_size: usize,
) -> (LocalStorageBackend, Uuid, Vec<Uuid>) {
    let local_id = Uuid::new_v4();
    let mut nodes = vec![local_id];
    for _ in 1..cluster_size {
        nodes.push(Uuid::new_v4());
    }
    let membership = Arc::new(MockCluster::new(local_id, nodes.clone()));
    let backend =
        LocalStorageBackend::with_cluster_membership(base.clone(), local_id, 500, membership);
    (backend, local_id, nodes)
}

// ============================================================================
// TC-286 — Storage intent tier quorum replicates to majority of nodes
// ============================================================================
/// Scenario: allocate volumes with each storage tier on clusters of varying
/// sizes and verify the replication behaviour:
///   - Quorum tier replicates to exactly ⌊N/2⌋+1 nodes (a strict majority)
///   - Local tier replicates to exactly 1 node (the local node)
///   - FullReplication tier replicates to all N nodes
///   - None tier replicates to exactly 1 node
///   - Performance tiers (Fast, Archive, Standard) do not affect replication count
#[tokio::test]
async fn tc286_storage_intent_tier_quorum_replicates_to_majority_of_nodes() {
    // --- 3-node cluster: quorum = 2 ---
    let base = temp_base_path();
    let (backend, _local_id, _nodes) = make_cluster_backend(&base, 3);

    let handle = backend
        .allocate_volume(
            &make_volume_iri("q3"),
            1,
            &intent(DurabilityTier::Quorum, PerformanceTier::Standard),
            &VolumeType::Mounted,
        )
        .await
        .expect("quorum allocation on 3-node cluster");

    assert_eq!(
        handle.replicated_to.len(),
        2,
        "quorum of 3 nodes must be 2 (3/2 + 1)"
    );

    // --- 5-node cluster: quorum = 3 ---
    let base5 = temp_base_path();
    let (backend5, _, _nodes5) = make_cluster_backend(&base5, 5);

    let handle5 = backend5
        .allocate_volume(
            &make_volume_iri("q5"),
            1,
            &intent(DurabilityTier::Quorum, PerformanceTier::Standard),
            &VolumeType::Mounted,
        )
        .await
        .expect("quorum allocation on 5-node cluster");

    assert_eq!(
        handle5.replicated_to.len(),
        3,
        "quorum of 5 nodes must be 3 (5/2 + 1)"
    );

    // --- 7-node cluster: quorum = 4 ---
    let base7 = temp_base_path();
    let (backend7, _, _) = make_cluster_backend(&base7, 7);

    let handle7 = backend7
        .allocate_volume(
            &make_volume_iri("q7"),
            1,
            &intent(DurabilityTier::Quorum, PerformanceTier::Standard),
            &VolumeType::Mounted,
        )
        .await
        .expect("quorum allocation on 7-node cluster");

    assert_eq!(
        handle7.replicated_to.len(),
        4,
        "quorum of 7 nodes must be 4 (7/2 + 1)"
    );

    // --- 1-node cluster: quorum = 1 ---
    let base1 = temp_base_path();
    let (backend1, _, _) = make_cluster_backend(&base1, 1);

    let handle1 = backend1
        .allocate_volume(
            &make_volume_iri("q1"),
            1,
            &intent(DurabilityTier::Quorum, PerformanceTier::Standard),
            &VolumeType::Mounted,
        )
        .await
        .expect("quorum allocation on 1-node cluster");

    assert_eq!(
        handle1.replicated_to.len(),
        1,
        "quorum of 1 node must be 1 (1/2 + 1)"
    );

    // --- Local tier: always 1 node regardless of cluster size ---
    let base_local = temp_base_path();
    let (backend_local, local_id_l, _) = make_cluster_backend(&base_local, 5);

    let handle_local = backend_local
        .allocate_volume(
            &make_volume_iri("local-vol"),
            1,
            &intent(DurabilityTier::Local, PerformanceTier::Standard),
            &VolumeType::Mounted,
        )
        .await
        .expect("local allocation");

    assert_eq!(
        handle_local.replicated_to.len(),
        1,
        "Local tier must replicate to exactly 1 node"
    );
    assert_eq!(
        handle_local.replicated_to[0], local_id_l,
        "Local tier must replicate to the local node"
    );

    // --- None tier: always 1 node ---
    let base_none = temp_base_path();
    let (backend_none, local_id_n, _) = make_cluster_backend(&base_none, 5);

    let handle_none = backend_none
        .allocate_volume(
            &make_volume_iri("none-vol"),
            1,
            &intent(DurabilityTier::None, PerformanceTier::Standard),
            &VolumeType::Mounted,
        )
        .await
        .expect("none allocation");

    assert_eq!(
        handle_none.replicated_to.len(),
        1,
        "None tier must replicate to exactly 1 node"
    );
    assert_eq!(
        handle_none.replicated_to[0], local_id_n,
        "None tier must replicate to the local node"
    );

    // --- FullReplication: all nodes ---
    let base_full = temp_base_path();
    let (backend_full, _, nodes_full) = make_cluster_backend(&base_full, 5);

    let handle_full = backend_full
        .allocate_volume(
            &make_volume_iri("full-vol"),
            1,
            &intent(DurabilityTier::FullReplication, PerformanceTier::Standard),
            &VolumeType::Mounted,
        )
        .await
        .expect("full replication allocation");

    assert_eq!(
        handle_full.replicated_to.len(),
        5,
        "FullReplication must replicate to all 5 nodes"
    );
    for node in &nodes_full {
        assert!(
            handle_full.replicated_to.contains(node),
            "FullReplication must include node {}",
            node
        );
    }

    // --- Performance tiers do not affect replication target count ---
    let base_perf = temp_base_path();
    let (backend_perf, _, _) = make_cluster_backend(&base_perf, 5);

    let handle_fast = backend_perf
        .allocate_volume(
            &make_volume_iri("fast-vol"),
            1,
            &intent(DurabilityTier::Quorum, PerformanceTier::Fast),
            &VolumeType::Mounted,
        )
        .await
        .expect("quorum + fast allocation");

    assert_eq!(
        handle_fast.replicated_to.len(),
        3,
        "Performance tier Fast must not change quorum replication count"
    );

    let handle_archive = backend_perf
        .allocate_volume(
            &make_volume_iri("archive-vol"),
            1,
            &intent(DurabilityTier::Quorum, PerformanceTier::Archive),
            &VolumeType::Mounted,
        )
        .await
        .expect("quorum + archive allocation");

    assert_eq!(
        handle_archive.replicated_to.len(),
        3,
        "Performance tier Archive must not change quorum replication count"
    );

    // Cleanup
    for p in [
        &base, &base5, &base7, &base1, &base_local, &base_none, &base_full, &base_perf,
    ] {
        let _ = std::fs::remove_dir_all(p);
    }
}

// ============================================================================
// TC-343 — Storage tiers exit — quorum tier replicates to majority
// ============================================================================
/// Exit criterion: for every even and odd cluster size from 1 to 9, verify
/// that quorum replication targets contain strictly more than half the
/// cluster nodes (i.e. ⌊N/2⌋+1). This is the gate check — it exhaustively
/// confirms the majority invariant that the platform relies on for data safety.
///
/// Additionally verifies:
///   - All tier + performance combinations serialize/deserialize correctly
///   - Quorum targets are always a subset of the full member list
///   - The quorum count satisfies: count > total_nodes / 2
#[tokio::test]
async fn tc343_storage_tiers_exit_quorum_tier_replicates_to_majority() {
    // --- Exhaustive quorum majority check for cluster sizes 1..=9 ---
    for cluster_size in 1..=9usize {
        let base = temp_base_path();
        let (backend, _, nodes) = make_cluster_backend(&base, cluster_size);

        let handle = backend
            .allocate_volume(
                &make_volume_iri(&format!("quorum-exit-{}", cluster_size)),
                1,
                &intent(DurabilityTier::Quorum, PerformanceTier::Standard),
                &VolumeType::Mounted,
            )
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "quorum allocation failed for cluster_size={}: {:?}",
                    cluster_size, e
                )
            });

        let expected_quorum = (cluster_size / 2) + 1;
        assert_eq!(
            handle.replicated_to.len(),
            expected_quorum,
            "cluster_size={}: quorum must be {} (N/2 + 1), got {}",
            cluster_size,
            expected_quorum,
            handle.replicated_to.len()
        );

        // The quorum count must be strictly more than half the cluster
        assert!(
            handle.replicated_to.len() > cluster_size / 2,
            "cluster_size={}: quorum {} must be > half ({})",
            cluster_size,
            handle.replicated_to.len(),
            cluster_size / 2
        );

        // All quorum targets must be valid cluster members
        for target in &handle.replicated_to {
            assert!(
                nodes.contains(target),
                "cluster_size={}: quorum target {} is not a cluster member",
                cluster_size,
                target
            );
        }

        let _ = std::fs::remove_dir_all(&base);
    }

    // --- All durability × performance combinations serde round-trip ---
    let durability_tiers = [
        DurabilityTier::FullReplication,
        DurabilityTier::Quorum,
        DurabilityTier::Local,
        DurabilityTier::None,
    ];
    let performance_tiers = [
        PerformanceTier::Standard,
        PerformanceTier::Fast,
        PerformanceTier::Archive,
    ];

    for d in &durability_tiers {
        for p in &performance_tiers {
            let si = StorageIntent {
                durability: d.clone(),
                performance: p.clone(),
                snapshots: None,
                offsite: None,
            };
            let json = serde_json::to_string(&si)
                .unwrap_or_else(|e| panic!("serialize {:?}×{:?} failed: {}", d, p, e));
            let back: StorageIntent = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("deserialize {:?}×{:?} failed: {}", d, p, e));

            // Verify the round-trip preserved the variant names
            let d_json = serde_json::to_string(&d).unwrap();
            let d_back_json = serde_json::to_string(&back.durability).unwrap();
            assert_eq!(d_json, d_back_json, "durability tier round-trip mismatch");

            let p_json = serde_json::to_string(&p).unwrap();
            let p_back_json = serde_json::to_string(&back.performance).unwrap();
            assert_eq!(p_json, p_back_json, "performance tier round-trip mismatch");
        }
    }

    // --- Quorum with even cluster size (4 nodes): quorum = 3 ---
    let base_even = temp_base_path();
    let (backend_even, _, nodes_even) = make_cluster_backend(&base_even, 4);

    let handle_even = backend_even
        .allocate_volume(
            &make_volume_iri("quorum-even"),
            1,
            &intent(DurabilityTier::Quorum, PerformanceTier::Standard),
            &VolumeType::Mounted,
        )
        .await
        .expect("quorum on even cluster");

    assert_eq!(
        handle_even.replicated_to.len(),
        3,
        "quorum of 4 nodes must be 3"
    );
    assert!(
        handle_even.replicated_to.len() > nodes_even.len() / 2,
        "quorum must be strictly more than half"
    );

    let _ = std::fs::remove_dir_all(&base_even);

    // --- Quorum with raw block volumes works the same way ---
    let base_raw = temp_base_path();
    let (backend_raw, _, _) = make_cluster_backend(&base_raw, 5);

    let handle_raw = backend_raw
        .allocate_volume(
            &make_volume_iri("quorum-raw"),
            1,
            &intent(DurabilityTier::Quorum, PerformanceTier::Fast),
            &VolumeType::RawBlock,
        )
        .await
        .expect("quorum + raw block allocation");

    assert_eq!(
        handle_raw.replicated_to.len(),
        3,
        "quorum replication must work identically for raw block volumes"
    );

    let _ = std::fs::remove_dir_all(&base_raw);
}
