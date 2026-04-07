/// PiCloud Server — Composition Root
///
/// This is the only binary that imports all slices.
/// Its only job is to instantiate implementations and wire them together.
/// No business logic lives here — it all lives in slices.
///
/// ADR-034: Vertical slice architecture with stable domain dependency

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use tracing::{error, info};
use uuid::Uuid;

use picloud_domain::iri::ClusterDomain;
use picloud_domain::traits::{
    ClusterMembership, DnsRegistry, EventFilter, EventLog, IdentityProvider, StateProjector,
    StorageBackend, WorkloadScheduler,
};

use picloud_cluster::{ClusterConfig, MdnsCluster};
use picloud_events::InMemoryEventLog;
use picloud_iam::LocalIdentityProvider;
use picloud_network::{InMemoryDnsRegistry, PlatformCa};
use picloud_rdf::OxigraphProjector;
use picloud_storage::LocalStorageBackend;
use picloud_workload::ProcessScheduler;
use picloud_http::PiCloudHttpServer;

/// Server configuration, loaded from env or defaults
struct ServerConfig {
    node_id: Uuid,
    node_name: String,
    cluster_domain: ClusterDomain,
    http_port: u16,
    bind_addr: IpAddr,
    storage_path: PathBuf,
    storage_capacity_gb: u64,
}

impl ServerConfig {
    fn from_env() -> Self {
        let node_id = std::env::var("PICLOUD_NODE_ID")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(Uuid::new_v4);

        let node_name = std::env::var("PICLOUD_NODE_NAME")
            .unwrap_or_else(|_| hostname().unwrap_or_else(|| format!("pi-{}", &node_id.to_string()[..8])));

        let cluster_domain = std::env::var("PICLOUD_DOMAIN")
            .map(ClusterDomain)
            .unwrap_or_default();

        let http_port = std::env::var("PICLOUD_HTTP_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(7443);

        let bind_addr = std::env::var("PICLOUD_BIND_ADDR")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

        let storage_path = std::env::var("PICLOUD_STORAGE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/var/lib/picloud/storage"));

        let storage_capacity_gb = std::env::var("PICLOUD_STORAGE_CAPACITY_GB")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);

        Self {
            node_id,
            node_name,
            cluster_domain,
            http_port,
            bind_addr,
            storage_path,
            storage_capacity_gb,
        }
    }
}

fn hostname() -> Option<String> {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("picloud=info".parse()?),
        )
        .json()
        .init();

    let config = ServerConfig::from_env();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        node_id = %config.node_id,
        node_name = %config.node_name,
        domain = %config.cluster_domain.0,
        "PiCloud server starting"
    );

    // Phase 1 bootstrap order:

    // 1. Start mDNS discovery and cluster membership
    let cluster_config = ClusterConfig {
        node_id: config.node_id,
        node_name: config.node_name.clone(),
        http_port: config.http_port,
        bind_addr: config.bind_addr,
        cluster_domain: config.cluster_domain.clone(),
    };
    let cluster = MdnsCluster::start(cluster_config)?;
    let cluster: Arc<dyn ClusterMembership> = Arc::new(cluster);
    info!("Cluster membership started");

    // 2. Start event log
    let event_log = Arc::new(InMemoryEventLog::new());
    let event_log_trait: Arc<dyn EventLog> = event_log.clone();
    info!("Event log started");

    // 3. Start RDF projector
    let projector = Arc::new(
        OxigraphProjector::with_domain(config.cluster_domain.clone())
            .expect("failed to initialize RDF projector"),
    );
    let projector_trait: Arc<dyn StateProjector> = projector.clone();
    info!("RDF projector started");

    // 4. Wire event log → RDF projector (projection loop)
    {
        let projector = projector_trait.clone();
        let mut rx = event_log_trait
            .subscribe(EventFilter::default())
            .await
            .expect("failed to subscribe to event log");

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let Err(e) = projector.project(&event).await {
                            error!(
                                event_type = %event.event_type,
                                error = %e,
                                "Failed to project event into RDF graph"
                            );
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        error!(skipped = n, "Projection loop lagged — events may be missing from graph");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("Event log closed — projection loop stopping");
                        break;
                    }
                }
            }
        });
        info!("Event projection loop started");
    }

    // 5. Start IAM service
    let iam_key = config.node_id.as_bytes();
    let iam = LocalIdentityProvider::new(iam_key, config.cluster_domain.clone());
    let iam: Arc<dyn IdentityProvider> = Arc::new(iam);
    info!("IAM service started");

    // 6. Start storage backend
    let storage = LocalStorageBackend::new(
        config.storage_path.clone(),
        config.node_id,
        config.storage_capacity_gb,
    );
    let storage: Arc<dyn StorageBackend> = Arc::new(storage);
    info!(
        path = %config.storage_path.display(),
        capacity_gb = config.storage_capacity_gb,
        "Storage backend started"
    );

    // 7. Start workload scheduler
    let scheduler = ProcessScheduler::new(config.node_id, config.cluster_domain.clone());
    let scheduler: Arc<dyn WorkloadScheduler> = Arc::new(scheduler);
    info!("Workload scheduler started");

    // 8. Start network/DNS/TLS
    let dns = InMemoryDnsRegistry::new();
    let dns: Arc<dyn DnsRegistry> = Arc::new(dns);
    let _ca = PlatformCa::new()?;
    info!("Network services started (DNS + CA)");

    // 9. Register this node's DNS entry
    let node_iri = picloud_domain::iri::IriBuilder::new(config.cluster_domain.clone())
        .node(&config.node_name);
    dns.register(
        &node_iri,
        &format!("{}:{}", config.bind_addr, config.http_port),
    )
    .await?;

    // 10. Emit NodeJoined event for this node
    {
        let iri_builder = picloud_domain::iri::IriBuilder::new(config.cluster_domain.clone());
        let node_joined = picloud_domain::events::EventEnvelope::new(
            iri_builder.event_schema("NodeJoined", 1),
            "NodeJoined",
            node_iri.clone(),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "node_id": config.node_id.to_string(),
                "node_name": config.node_name,
                "node_iri": node_iri.as_str(),
                "address": format!("{}:{}", config.bind_addr, config.http_port),
            }),
        );
        event_log_trait.append(node_joined).await?;
        info!("NodeJoined event emitted");
    }

    // 11. Start HTTP server
    let http_addr = SocketAddr::new(config.bind_addr, config.http_port);
    let http_server = PiCloudHttpServer::new(http_addr, config.cluster_domain.clone())
        .with_dependencies(
            event_log_trait.clone(),
            projector_trait.clone(),
            cluster.clone(),
            iam.clone(),
            storage.clone(),
            scheduler.clone(),
        );

    // Log startup summary
    let members = cluster.members().await?;
    info!(
        node_count = members.len(),
        is_leader = cluster.is_leader().await,
        "PiCloud server ready"
    );

    // Run HTTP server until shutdown signal
    tokio::select! {
        result = http_server.start() => {
            if let Err(e) = result {
                error!(error = %e, "HTTP server failed");
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Shutdown signal received");
        }
    }

    info!("Shutting down");
    Ok(())
}
