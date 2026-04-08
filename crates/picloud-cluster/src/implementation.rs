//! ClusterMembership trait implementation backed by mDNS discovery.

use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;
use tracing::info;
use uuid::Uuid;

use picloud_domain::error::Result;
use picloud_domain::iri::{ClusterDomain, IriBuilder};
use picloud_domain::traits::{ClusterMembership, NodeInfo};

use crate::discovery::{DiscoveryConfig, MdnsDiscovery};
use crate::peers::PeerList;
use crate::raft::{node_id_from_uuid, PiCloudRaft};

/// Configuration for the cluster node.
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    pub node_id: Uuid,
    pub node_name: String,
    pub http_port: u16,
    pub bind_addr: IpAddr,
    pub cluster_domain: ClusterDomain,
}

/// Concrete implementation of ClusterMembership.
///
/// Uses mDNS to discover peers on the local network and maintains a live
/// peer list. Leadership is determined by the Raft consensus protocol.
pub struct MdnsCluster {
    config: ClusterConfig,
    peers: Arc<PeerList>,
    discovery: MdnsDiscovery,
    iri_builder: IriBuilder,
    _browse_handle: Option<tokio::task::JoinHandle<()>>,
    _monitor_handle: Option<tokio::task::JoinHandle<()>>,
    /// Raft node handle — `None` until `set_raft` is called.
    raft: Option<Arc<PiCloudRaft>>,
}

impl MdnsCluster {
    /// Create and start the cluster node.
    ///
    /// This immediately registers the node via mDNS and begins browsing
    /// for peers on the local network.
    pub fn start(config: ClusterConfig) -> Result<Self> {
        let peers = Arc::new(PeerList::new());
        let iri_builder = IriBuilder::new(config.cluster_domain.clone());

        let discovery_config = DiscoveryConfig {
            node_id: config.node_id,
            node_name: config.node_name.clone(),
            http_port: config.http_port,
            bind_addr: config.bind_addr,
        };

        let discovery = MdnsDiscovery::new(discovery_config, Arc::clone(&peers))?;
        let browse_handle = discovery.start_browsing()?;
        let monitor_handle = discovery.start_daemon_monitor()?;

        info!(
            node_id = %config.node_id,
            node_name = %config.node_name,
            http_port = config.http_port,
            "Cluster node started, browsing for peers"
        );

        Ok(Self {
            config,
            peers,
            discovery,
            iri_builder,
            _browse_handle: Some(browse_handle),
            _monitor_handle: Some(monitor_handle),
            raft: None,
        })
    }

    /// Access the peer list directly.
    pub fn peers(&self) -> &Arc<PeerList> {
        &self.peers
    }

    /// Set the Raft handle after it has been created.
    ///
    /// This is called from the composition root after Raft is initialised.
    pub fn set_raft(&mut self, raft: Arc<PiCloudRaft>) {
        self.raft = Some(raft);
    }

    /// Return a reference to the Raft handle, if set.
    pub fn raft(&self) -> Option<&Arc<PiCloudRaft>> {
        self.raft.as_ref()
    }

    /// Return the `u64` Raft node ID derived from this node's UUID.
    pub fn raft_node_id(&self) -> u64 {
        node_id_from_uuid(&self.config.node_id)
    }

    /// Gracefully shut down mDNS and stop browsing.
    pub fn shutdown(&self) -> Result<()> {
        self.discovery.shutdown()
    }
}

#[async_trait]
impl ClusterMembership for MdnsCluster {
    async fn is_leader(&self) -> bool {
        if let Some(ref raft) = self.raft {
            let my_id = self.raft_node_id();
            raft.current_leader().await == Some(my_id)
        } else {
            // Fallback when Raft is not yet initialised
            let peers = self.peers.all();
            if peers.is_empty() {
                return true;
            }
            peers.iter().all(|p| self.config.node_id < p.node_id)
        }
    }

    async fn leader_id(&self) -> Result<Uuid> {
        if let Some(ref raft) = self.raft {
            if let Some(_leader_raft_id) = raft.current_leader().await {
                // In a real multi-node setup we would map the raft u64 ID
                // back to a UUID. For now, if the Raft leader is us, return
                // our UUID; otherwise fall through to the peer-based lookup.
                let my_id = self.raft_node_id();
                if _leader_raft_id == my_id {
                    return Ok(self.config.node_id);
                }
                // Look up the leader UUID from peer list by matching raft node ID
                for peer in self.peers.all() {
                    if node_id_from_uuid(&peer.node_id) == _leader_raft_id {
                        return Ok(peer.node_id);
                    }
                }
            }
        }

        // Fallback: lowest UUID
        let peers = self.peers.all();
        if peers.is_empty() {
            return Ok(self.config.node_id);
        }
        let min_peer = peers.iter().min_by_key(|p| p.node_id).unwrap();
        if self.config.node_id < min_peer.node_id {
            Ok(self.config.node_id)
        } else {
            Ok(min_peer.node_id)
        }
    }

    async fn members(&self) -> Result<Vec<NodeInfo>> {
        let leader = self.leader_id().await?;
        let mut members = Vec::new();

        // Add self
        members.push(NodeInfo {
            node_id: self.config.node_id,
            node_iri: self.iri_builder.node(&self.config.node_name),
            address: format!("{}:{}", self.config.bind_addr, self.config.http_port),
            is_leader: self.config.node_id == leader,
        });

        // Add discovered peers
        for peer in self.peers.all() {
            members.push(NodeInfo {
                node_id: peer.node_id,
                node_iri: self.iri_builder.node(&peer.node_name),
                address: peer.address.clone(),
                is_leader: peer.node_id == leader,
            });
        }

        Ok(members)
    }

    async fn local_node_id(&self) -> Uuid {
        self.config.node_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn test_config() -> ClusterConfig {
        ClusterConfig {
            node_id: Uuid::new_v4(),
            node_name: "test-node".to_string(),
            http_port: 7443,
            bind_addr: IpAddr::V4(Ipv4Addr::LOCALHOST),
            cluster_domain: ClusterDomain::default(),
        }
    }

    #[tokio::test]
    async fn test_single_node_is_leader() {
        let config = test_config();
        let peers = Arc::new(PeerList::new());
        let iri_builder = IriBuilder::new(config.cluster_domain.clone());

        // Build without mDNS for unit testing
        let cluster = MdnsClusterTestHarness {
            config: config.clone(),
            peers,
            iri_builder,
        };

        assert!(cluster.is_leader().await);
        assert_eq!(cluster.leader_id().await.unwrap(), config.node_id);
        assert_eq!(cluster.local_node_id().await, config.node_id);
    }

    #[tokio::test]
    async fn test_members_includes_self() {
        let config = test_config();
        let peers = Arc::new(PeerList::new());
        let iri_builder = IriBuilder::new(config.cluster_domain.clone());

        let cluster = MdnsClusterTestHarness {
            config,
            peers,
            iri_builder,
        };

        let members = cluster.members().await.unwrap();
        assert_eq!(members.len(), 1);
        assert!(members[0].is_leader);
    }

    #[tokio::test]
    async fn test_members_includes_peers() {
        let config = test_config();
        let peers = Arc::new(PeerList::new());
        let iri_builder = IriBuilder::new(config.cluster_domain.clone());

        peers.add(crate::peers::PeerInfo {
            node_id: Uuid::new_v4(),
            node_name: "peer-1".to_string(),
            address: "192.168.1.20:7443".to_string(),
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)),
            port: 7443,
        });

        let cluster = MdnsClusterTestHarness {
            config,
            peers,
            iri_builder,
        };

        let members = cluster.members().await.unwrap();
        assert_eq!(members.len(), 2);
        // Exactly one leader
        assert_eq!(members.iter().filter(|m| m.is_leader).count(), 1);
    }

    /// Test harness that implements ClusterMembership without needing mDNS.
    struct MdnsClusterTestHarness {
        config: ClusterConfig,
        peers: Arc<PeerList>,
        iri_builder: IriBuilder,
    }

    #[async_trait]
    impl ClusterMembership for MdnsClusterTestHarness {
        async fn is_leader(&self) -> bool {
            let peers = self.peers.all();
            if peers.is_empty() {
                return true;
            }
            peers.iter().all(|p| self.config.node_id < p.node_id)
        }

        async fn leader_id(&self) -> Result<Uuid> {
            let peers = self.peers.all();
            if peers.is_empty() {
                return Ok(self.config.node_id);
            }
            let min_peer = peers.iter().min_by_key(|p| p.node_id).unwrap();
            if self.config.node_id < min_peer.node_id {
                Ok(self.config.node_id)
            } else {
                Ok(min_peer.node_id)
            }
        }

        async fn members(&self) -> Result<Vec<NodeInfo>> {
            let leader = self.leader_id().await?;
            let mut members = vec![NodeInfo {
                node_id: self.config.node_id,
                node_iri: self.iri_builder.node(&self.config.node_name),
                address: format!("{}:{}", self.config.bind_addr, self.config.http_port),
                is_leader: self.config.node_id == leader,
            }];
            for peer in self.peers.all() {
                members.push(NodeInfo {
                    node_id: peer.node_id,
                    node_iri: self.iri_builder.node(&peer.node_name),
                    address: peer.address.clone(),
                    is_leader: peer.node_id == leader,
                });
            }
            Ok(members)
        }

        async fn local_node_id(&self) -> Uuid {
            self.config.node_id
        }
    }
}
