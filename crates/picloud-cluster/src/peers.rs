//! Peer List
//!
//! Thread-safe registry of discovered cluster peers. Updated by the mDNS
//! browser and read by the ClusterMembership trait implementation.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::RwLock;

use tracing::{debug, info};
use uuid::Uuid;

/// Information about a discovered peer node.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub node_id: Uuid,
    pub node_name: String,
    /// `ip:port` address for HTTP communication
    pub address: String,
    pub ip: IpAddr,
    pub port: u16,
}

/// Thread-safe peer list maintained by mDNS discovery.
pub struct PeerList {
    peers: RwLock<HashMap<Uuid, PeerInfo>>,
}

impl PeerList {
    pub fn new() -> Self {
        Self {
            peers: RwLock::new(HashMap::new()),
        }
    }

    /// Add or update a peer. Returns true if this is a new peer.
    pub fn add(&self, peer: PeerInfo) -> bool {
        let mut peers = self.peers.write().expect("peer list lock poisoned");
        let is_new = !peers.contains_key(&peer.node_id);
        if is_new {
            info!(node_id = %peer.node_id, node_name = %peer.node_name, "New peer added");
        } else {
            debug!(node_id = %peer.node_id, "Peer updated");
        }
        peers.insert(peer.node_id, peer);
        is_new
    }

    /// Remove a peer by node ID.
    pub fn remove(&self, node_id: &Uuid) -> Option<PeerInfo> {
        let mut peers = self.peers.write().expect("peer list lock poisoned");
        let removed = peers.remove(node_id);
        if let Some(ref p) = removed {
            info!(node_id = %p.node_id, node_name = %p.node_name, "Peer removed");
        }
        removed
    }

    /// Remove a peer by mDNS fullname (best-effort match on node_name).
    pub fn remove_by_fullname(&self, fullname: &str) {
        let mut peers = self.peers.write().expect("peer list lock poisoned");
        // fullname looks like "pi-node-01._picloud._tcp.local."
        // Extract the instance name (before the first '.')
        let instance = fullname
            .split('.')
            .next()
            .unwrap_or(fullname);

        let to_remove: Vec<Uuid> = peers
            .iter()
            .filter(|(_, p)| p.node_name == instance)
            .map(|(id, _)| *id)
            .collect();

        for id in to_remove {
            if let Some(p) = peers.remove(&id) {
                info!(
                    node_id = %p.node_id,
                    node_name = %p.node_name,
                    "Peer removed (mDNS service gone)"
                );
            }
        }
    }

    /// Get a snapshot of all current peers.
    pub fn all(&self) -> Vec<PeerInfo> {
        let peers = self.peers.read().expect("peer list lock poisoned");
        peers.values().cloned().collect()
    }

    /// Get a specific peer by node ID.
    pub fn get(&self, node_id: &Uuid) -> Option<PeerInfo> {
        let peers = self.peers.read().expect("peer list lock poisoned");
        peers.get(node_id).cloned()
    }

    /// Number of known peers (not including self).
    pub fn len(&self) -> usize {
        let peers = self.peers.read().expect("peer list lock poisoned");
        peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Check if a node is a known peer.
    pub fn contains(&self, node_id: &Uuid) -> bool {
        let peers = self.peers.read().expect("peer list lock poisoned");
        peers.contains_key(node_id)
    }
}

impl Default for PeerList {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn make_peer(name: &str) -> PeerInfo {
        PeerInfo {
            node_id: Uuid::new_v4(),
            node_name: name.to_string(),
            address: format!("192.168.1.10:7443"),
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            port: 7443,
        }
    }

    #[test]
    fn test_add_and_get() {
        let list = PeerList::new();
        let peer = make_peer("pi-node-01");
        let id = peer.node_id;

        assert!(list.add(peer));
        assert_eq!(list.len(), 1);
        assert!(list.contains(&id));

        let got = list.get(&id).unwrap();
        assert_eq!(got.node_name, "pi-node-01");
    }

    #[test]
    fn test_add_duplicate_returns_false() {
        let list = PeerList::new();
        let peer = make_peer("pi-node-01");
        let dup = peer.clone();

        assert!(list.add(peer));
        assert!(!list.add(dup));
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_remove() {
        let list = PeerList::new();
        let peer = make_peer("pi-node-01");
        let id = peer.node_id;

        list.add(peer);
        assert!(list.remove(&id).is_some());
        assert!(list.is_empty());
    }

    #[test]
    fn test_remove_by_fullname() {
        let list = PeerList::new();
        let peer = make_peer("pi-node-01");

        list.add(peer);
        assert_eq!(list.len(), 1);

        list.remove_by_fullname("pi-node-01._picloud._tcp.local.");
        assert!(list.is_empty());
    }

    #[test]
    fn test_all() {
        let list = PeerList::new();
        list.add(make_peer("a"));
        list.add(make_peer("b"));
        list.add(make_peer("c"));
        assert_eq!(list.all().len(), 3);
    }
}
