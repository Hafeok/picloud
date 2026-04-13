//! mDNS Service Discovery
//!
//! Each node broadcasts a `_picloud._tcp.local.` service on startup via mdns-sd.
//! The daemon continuously browses for peers and feeds discovered/removed nodes
//! into the shared peer list.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use mdns_sd::{DaemonEvent, ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::watch;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::peers::{PeerInfo, PeerList};

/// Default service type when no domain scoping is applied.
const DEFAULT_SERVICE_TYPE: &str = "_picloud._tcp.local.";
const PROP_NODE_ID: &str = "node_id";
const PROP_HTTP_PORT: &str = "http_port";
const PROP_CLUSTER_ID: &str = "cluster_id";

/// Build a domain-scoped mDNS service type (ADR-042).
///
/// Nodes advertising different domains are mutually invisible on the network.
/// The domain is hashed to stay within DNS label length limits.
pub fn service_type_for_domain(domain: &str) -> String {
    if domain == "picloud.local" || domain.is_empty() {
        DEFAULT_SERVICE_TYPE.to_string()
    } else {
        // Use a short hash of the domain to create a unique but valid DNS service type.
        // DNS labels are max 63 chars; we use the first 8 hex chars of a simple hash.
        let hash = {
            let mut h: u64 = 5381;
            for b in domain.bytes() {
                h = h.wrapping_mul(33).wrapping_add(b as u64);
            }
            format!("{:016x}", h)
        };
        format!("_pc-{}._tcp.local.", &hash[..8])
    }
}

/// Configuration for mDNS discovery.
#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    pub node_id: Uuid,
    pub node_name: String,
    pub http_port: u16,
    pub bind_addr: IpAddr,
    /// Cluster domain — used to scope mDNS service type (ADR-042).
    /// If None, defaults to "picloud.local".
    pub cluster_domain: Option<String>,
    /// Cluster ID — advertised in mDNS TXT properties for cross-validation.
    pub cluster_id: Option<Uuid>,
}

/// Manages mDNS registration and browsing.
pub struct MdnsDiscovery {
    config: DiscoveryConfig,
    daemon: ServiceDaemon,
    peers: Arc<PeerList>,
    shutdown_tx: watch::Sender<bool>,
    /// Domain-scoped service type string (ADR-042)
    service_type: String,
}

impl MdnsDiscovery {
    /// Create a new discovery instance and register this node's service.
    pub fn new(config: DiscoveryConfig, peers: Arc<PeerList>) -> picloud_domain::error::Result<Self> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| picloud_domain::error::PiCloudError::Internal(
                format!("Failed to create mDNS daemon: {e}"),
            ))?;

        let (shutdown_tx, _) = watch::channel(false);

        // Compute domain-scoped service type (ADR-042)
        let service_type = match &config.cluster_domain {
            Some(domain) => service_type_for_domain(domain),
            None => DEFAULT_SERVICE_TYPE.to_string(),
        };

        let mut discovery = Self {
            config,
            daemon,
            peers,
            shutdown_tx,
            service_type,
        };

        discovery.register_self()?;
        Ok(discovery)
    }

    /// Register this node as an mDNS service.
    fn register_self(&mut self) -> picloud_domain::error::Result<()> {
        let mut properties = HashMap::new();
        properties.insert(PROP_NODE_ID.to_string(), self.config.node_id.to_string());
        properties.insert(PROP_HTTP_PORT.to_string(), self.config.http_port.to_string());
        if let Some(ref cluster_id) = self.config.cluster_id {
            properties.insert(PROP_CLUSTER_ID.to_string(), cluster_id.to_string());
        }

        let host = format!("{}.local.", self.config.node_name);

        let service_info = ServiceInfo::new(
            &self.service_type,
            &self.config.node_name,
            &host,
            self.config.bind_addr,
            self.config.http_port,
            properties,
        )
        .map_err(|e| picloud_domain::error::PiCloudError::Internal(
            format!("Failed to create mDNS service info: {e}"),
        ))?;

        self.daemon
            .register(service_info)
            .map_err(|e| picloud_domain::error::PiCloudError::Internal(
                format!("Failed to register mDNS service: {e}"),
            ))?;

        info!(
            node_id = %self.config.node_id,
            node_name = %self.config.node_name,
            port = self.config.http_port,
            "Registered mDNS service"
        );

        Ok(())
    }

    /// Start browsing for peers. Spawns a background task that updates the peer list.
    /// Returns a JoinHandle that runs until shutdown is signalled.
    pub fn start_browsing(&self) -> picloud_domain::error::Result<tokio::task::JoinHandle<()>> {
        let receiver = self.daemon.browse(&self.service_type).map_err(|e| {
            picloud_domain::error::PiCloudError::Internal(
                format!("Failed to start mDNS browsing: {e}"),
            )
        })?;

        let peers = Arc::clone(&self.peers);
        let local_node_id = self.config.node_id;
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        info!("mDNS browser shutting down");
                        break;
                    }
                    event = tokio::task::spawn_blocking({
                        let receiver = receiver.clone();
                        move || receiver.recv()
                    }) => {
                        match event {
                            Ok(Ok(service_event)) => {
                                Self::handle_event(&peers, local_node_id, service_event);
                            }
                            Ok(Err(e)) => {
                                warn!("mDNS receive error: {e}");
                                break;
                            }
                            Err(e) => {
                                error!("mDNS blocking task panicked: {e}");
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(handle)
    }

    /// Monitor mDNS daemon events (for diagnostics).
    pub fn start_daemon_monitor(&self) -> picloud_domain::error::Result<tokio::task::JoinHandle<()>> {
        let monitor = self.daemon.monitor().map_err(|e| {
            picloud_domain::error::PiCloudError::Internal(
                format!("Failed to start mDNS monitor: {e}"),
            )
        })?;

        let mut shutdown_rx = self.shutdown_tx.subscribe();

        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => break,
                    event = tokio::task::spawn_blocking({
                        let monitor = monitor.clone();
                        move || monitor.recv()
                    }) => {
                        match event {
                            Ok(Ok(DaemonEvent::Error(e))) => {
                                warn!("mDNS daemon error: {:?}", e);
                            }
                            Ok(Ok(event)) => {
                                debug!("mDNS daemon event: {:?}", event);
                            }
                            Ok(Err(_)) | Err(_) => break,
                        }
                    }
                }
            }
        });

        Ok(handle)
    }

    /// Process an mDNS service event, updating the peer list accordingly.
    ///
    /// Public so integration tests can simulate mDNS events without requiring
    /// actual multicast networking.
    pub fn handle_event(peers: &PeerList, local_node_id: Uuid, event: ServiceEvent) {
        match event {
            ServiceEvent::ServiceResolved(info) => {
                let Some(node_id) = Self::extract_node_id(&info) else {
                    warn!(
                        fullname = info.get_fullname(),
                        "Resolved service missing node_id property, ignoring"
                    );
                    return;
                };

                // Don't add ourselves
                if node_id == local_node_id {
                    debug!("Ignoring self-discovery");
                    return;
                }

                let http_port = info
                    .get_property_val_str(PROP_HTTP_PORT)
                    .and_then(|s| s.parse::<u16>().ok())
                    .unwrap_or(info.get_port());

                // Pick the first address
                let addresses = info.get_addresses();
                let Some(addr) = addresses.iter().next() else {
                    warn!(node_id = %node_id, "Resolved service has no addresses");
                    return;
                };

                let peer = PeerInfo {
                    node_id,
                    node_name: info.get_hostname().trim_end_matches('.').to_string(),
                    address: format!("{}:{}", addr, http_port),
                    ip: *addr,
                    port: http_port,
                    last_seen: std::time::Instant::now(),
                };

                info!(
                    node_id = %peer.node_id,
                    node_name = %peer.node_name,
                    address = %peer.address,
                    "Discovered peer via mDNS"
                );

                peers.add(peer);
                // Refresh the last_seen timestamp so re-resolution resets the stale timer.
                peers.touch(&node_id);
            }
            ServiceEvent::ServiceRemoved(_, fullname) => {
                info!(fullname = %fullname, "Peer removed from mDNS");
                // We'll try to extract the node name and remove by matching
                peers.remove_by_fullname(&fullname);
            }
            ServiceEvent::SearchStarted(stype) => {
                debug!(service_type = %stype, "mDNS search started");
            }
            ServiceEvent::SearchStopped(stype) => {
                debug!(service_type = %stype, "mDNS search stopped");
            }
            _ => {}
        }
    }

    /// Extract node_id from mDNS service properties.
    pub fn extract_node_id(info: &ServiceInfo) -> Option<Uuid> {
        info.get_property_val_str(PROP_NODE_ID)
            .and_then(|s| s.parse::<Uuid>().ok())
    }

    /// Signal shutdown and unregister the service.
    pub fn shutdown(&self) -> picloud_domain::error::Result<()> {
        let _ = self.shutdown_tx.send(true);

        let fullname = format!("{}.{}", self.config.node_name, self.service_type);
        let _ = self.daemon.unregister(&fullname);

        info!(node_name = %self.config.node_name, "mDNS service unregistered");
        Ok(())
    }
}

impl Drop for MdnsDiscovery {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_service_type_format() {
        assert!(DEFAULT_SERVICE_TYPE.ends_with(".local."));
        assert!(DEFAULT_SERVICE_TYPE.starts_with('_'));
    }

    #[test]
    fn test_discovery_config() {
        let config = DiscoveryConfig {
            node_id: Uuid::new_v4(),
            node_name: "pi-node-01".to_string(),
            http_port: 7443,
            bind_addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            cluster_domain: None,
            cluster_id: None,
        };
        assert_eq!(config.http_port, 7443);
    }

    #[test]
    fn test_service_type_default_domain() {
        let st = service_type_for_domain("picloud.local");
        assert_eq!(st, DEFAULT_SERVICE_TYPE);
    }

    #[test]
    fn test_service_type_custom_domain() {
        let st = service_type_for_domain("acme.local");
        assert!(st.starts_with("_pc-"));
        assert!(st.ends_with("._tcp.local."));
        assert_ne!(st, DEFAULT_SERVICE_TYPE);
    }

    #[test]
    fn test_service_type_different_domains_differ() {
        let a = service_type_for_domain("acme.local");
        let b = service_type_for_domain("other.local");
        assert_ne!(a, b, "Different domains must produce different service types");
    }

    #[test]
    fn test_service_type_empty_domain_uses_default() {
        let st = service_type_for_domain("");
        assert_eq!(st, DEFAULT_SERVICE_TYPE);
    }
}
