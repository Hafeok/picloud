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

pub use discovery::{DiscoveryConfig, MdnsDiscovery};
pub use implementation::{ClusterConfig, MdnsCluster};
pub use peers::{PeerInfo, PeerList};
