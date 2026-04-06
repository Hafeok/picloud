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

const SERVICE_TYPE: &str = "_picloud._tcp.local.";
const PROP_NODE_ID: &str = "node_id";
const PROP_HTTP_PORT: &str = "http_port";

/// Configuration for mDNS discovery.
#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    pub node_id: Uuid,
    pub node_name: String,
    pub http_port: u16,
    pub bind_addr: IpAddr,
}

/// Manages mDNS registration and browsing.
pub struct MdnsDiscovery {
    config: DiscoveryConfig,
    daemon: ServiceDaemon,
    peers: Arc<PeerList>,
    shutdown_tx: watch::Sender<bool>,
}

impl MdnsDiscovery {
    /// Create a new discovery instance and register this node's service.
    pub fn new(config: DiscoveryConfig, peers: Arc<PeerList>) -> picloud_domain::error::Result<Self> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| picloud_domain::error::PiCloudError::Internal(
                format!("Failed to create mDNS daemon: {e}"),
            ))?;

        let (shutdown_tx, _) = watch::channel(false);

        let mut discovery = Self {
            config,
            daemon,
            peers,
            shutdown_tx,
        };

        discovery.register_self()?;
        Ok(discovery)
    }

    /// Register this node as an mDNS service.
    fn register_self(&mut self) -> picloud_domain::error::Result<()> {
        let mut properties = HashMap::new();
        properties.insert(PROP_NODE_ID.to_string(), self.config.node_id.to_string());
        properties.insert(PROP_HTTP_PORT.to_string(), self.config.http_port.to_string());

        let host = format!("{}.local.", self.config.node_name);

        let service_info = ServiceInfo::new(
            SERVICE_TYPE,
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
        let receiver = self.daemon.browse(SERVICE_TYPE).map_err(|e| {
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

    fn handle_event(peers: &PeerList, local_node_id: Uuid, event: ServiceEvent) {
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
                };

                info!(
                    node_id = %peer.node_id,
                    node_name = %peer.node_name,
                    address = %peer.address,
                    "Discovered peer via mDNS"
                );

                peers.add(peer);
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

    fn extract_node_id(info: &ServiceInfo) -> Option<Uuid> {
        info.get_property_val_str(PROP_NODE_ID)
            .and_then(|s| s.parse::<Uuid>().ok())
    }

    /// Signal shutdown and unregister the service.
    pub fn shutdown(&self) -> picloud_domain::error::Result<()> {
        let _ = self.shutdown_tx.send(true);

        let fullname = format!("{}.{}", self.config.node_name, SERVICE_TYPE);
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
        assert!(SERVICE_TYPE.ends_with(".local."));
        assert!(SERVICE_TYPE.starts_with('_'));
    }

    #[test]
    fn test_discovery_config() {
        let config = DiscoveryConfig {
            node_id: Uuid::new_v4(),
            node_name: "pi-node-01".to_string(),
            http_port: 7443,
            bind_addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        };
        assert_eq!(config.http_port, 7443);
    }
}
