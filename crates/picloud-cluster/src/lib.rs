//! picloud-cluster
//!
//! mDNS discovery and cluster membership for PiCloud nodes.
//!
//! Nodes broadcast a `_picloud._tcp.local.` service on startup, discover
//! peers via mDNS browsing, and maintain a live peer list. The
//! [`MdnsCluster`] type implements `ClusterMembership` from picloud-domain.
//!
//! Depends only on picloud-domain — never on other slices.
//! Slices communicate at runtime via the event log.

pub mod discovery;
pub mod implementation;
pub mod peers;
pub mod raft;

pub use discovery::{service_type_for_domain, DiscoveryConfig, MdnsDiscovery};
pub use implementation::{ClusterConfig, InMemoryClusterIdentityStore, MdnsCluster};
pub use peers::{PeerInfo, PeerList};
pub use raft::{
    create_persistent_raft_node, create_raft_node, node_id_from_uuid, raft_rpc_router,
    ApplyCallback, ClientRequest, ClientResponse, HttpNetworkFactory, MemLogStore,
    MemStateMachine, PiCloudRaft, PiCloudTypeConfig, SledLogStore, SledStateMachine,
};
