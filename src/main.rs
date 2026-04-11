/// PiCloud Server — Composition Root
///
/// This is the only binary that imports all slices.
/// Its only job is to instantiate implementations and wire them together.
/// No business logic lives here — it all lives in slices.
///
/// ADR-034: Vertical slice architecture with stable domain dependency

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Parser;
use serde::Deserialize;
use tracing::{error, info, warn};
use uuid::Uuid;

use picloud_domain::iri::ClusterDomain;
use picloud_domain::traits::{
    ClusterIdentityStore, ClusterMembership, DnsRegistry, EventFilter,
    EventLog, IdentityProvider, SecretStore, StateProjector, StorageBackend,
    WorkloadScheduler,
};

use picloud_cluster::{ClusterConfig, InMemoryClusterIdentityStore, MdnsCluster};
use picloud_events::{PersistentEventLog, WriteThroughEventLog};
use picloud_iam::{InMemorySecretStore, LocalIdentityProvider};
use picloud_network::{InMemoryDnsRegistry, PlatformCa};
use picloud_rdf::OxigraphProjector;
use picloud_registry::LocalRegistryBackend;
use picloud_storage::{LocalStorageBackend, StorageReplicator};
use picloud_workload::ProcessScheduler;
use picloud_http::{InferenceEngine, JsonlTelemetryStore, MetricsAgent, OtelAggregator, OtelStream, PiCloudHttpServer, Provisioner};

/// CLI arguments — only `--config` is supported; everything else
/// comes from the TOML file or environment variables.
#[derive(Parser, Debug)]
#[command(name = "picloud-server", about = "PiCloud server — runs on every node")]
struct CliArgs {
    /// Path to a TOML configuration file (optional).
    /// Env vars always override values from the file.
    #[arg(long = "config", value_name = "PATH")]
    config: Option<PathBuf>,
}

/// TOML-deserializable platform configuration (ADR-056).
///
/// All fields are optional — missing fields fall back to defaults.
/// Environment variables always override values loaded from the file.
#[derive(Debug, Default, Deserialize)]
struct PlatformConfig {
    /// Root directory for all PiCloud data.
    #[serde(default = "PlatformConfig::default_data_root")]
    data_root: Option<String>,

    /// HTTP/HTTPS listen port.
    http_port: Option<u16>,

    /// Authoritative DNS server port.
    dns_port: Option<u16>,

    /// Raft consensus port.
    raft_port: Option<u16>,

    /// Enrollment API port (new-node join).
    enrollment_port: Option<u16>,

    /// Cluster domain suffix.
    domain: Option<String>,

    /// Storage capacity limit (e.g. "200GB"). Parsed to a raw GB number.
    storage_limit: Option<String>,
}

impl PlatformConfig {
    fn default_data_root() -> Option<String> {
        Some("/var/lib/picloud".to_string())
    }

    /// Load from a TOML file, returning `Default` if the path is `None`.
    fn load(path: Option<&Path>) -> Self {
        let Some(path) = path else {
            return Self::default();
        };
        match std::fs::read_to_string(path) {
            Ok(contents) => match toml::from_str::<PlatformConfig>(&contents) {
                Ok(cfg) => {
                    info!(path = %path.display(), "Loaded TOML config");
                    cfg
                }
                Err(e) => {
                    eprintln!("WARN: Failed to parse TOML config {}: {e}", path.display());
                    Self::default()
                }
            },
            Err(e) => {
                eprintln!("WARN: Failed to read config file {}: {e}", path.display());
                Self::default()
            }
        }
    }

    /// Parse `storage_limit` like "200GB" / "100gb" into a u64 GB value.
    fn storage_limit_gb(&self) -> Option<u64> {
        let s = self.storage_limit.as_deref()?;
        let s = s.trim();
        let numeric = s.trim_end_matches(|c: char| c.is_ascii_alphabetic());
        numeric.trim().parse::<u64>().ok()
    }
}

/// Server configuration, loaded from TOML then env overrides.
struct ServerConfig {
    node_id: Uuid,
    node_name: String,
    cluster_domain: ClusterDomain,
    http_port: u16,
    dns_port: u16,
    raft_port: u16,
    enrollment_port: u16,
    bind_addr: IpAddr,
    events_path: PathBuf,
    storage_path: PathBuf,
    storage_capacity_gb: u64,
    rdf_path: PathBuf,
    registry_path: PathBuf,
}

impl ServerConfig {
    /// Build config with layered precedence: env > TOML > defaults.
    fn from_toml_and_env(toml: &PlatformConfig) -> Self {
        let data_root = std::env::var("PICLOUD_DATA_ROOT")
            .ok()
            .or_else(|| toml.data_root.clone())
            .unwrap_or_else(|| "/var/lib/picloud".to_string());

        // --- per-instance (env-only, never from TOML) ---
        let node_id = std::env::var("PICLOUD_NODE_ID")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(Uuid::new_v4);

        let node_name = std::env::var("PICLOUD_NODE_NAME")
            .unwrap_or_else(|_| hostname().unwrap_or_else(|| format!("pi-{}", &node_id.to_string()[..8])));

        // --- layered: env > TOML > default ---
        let cluster_domain = std::env::var("PICLOUD_DOMAIN")
            .ok()
            .or_else(|| toml.domain.clone())
            .map(ClusterDomain)
            .unwrap_or_default();

        let http_port = std::env::var("PICLOUD_HTTP_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .or(toml.http_port)
            .unwrap_or(7443);

        let dns_port = std::env::var("PICLOUD_DNS_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .or(toml.dns_port)
            .unwrap_or(53);

        let raft_port = std::env::var("PICLOUD_RAFT_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .or(toml.raft_port)
            .unwrap_or(2380);

        let enrollment_port = std::env::var("PICLOUD_ENROLLMENT_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .or(toml.enrollment_port)
            .unwrap_or(444);

        let bind_addr = std::env::var("PICLOUD_BIND_ADDR")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

        let events_path = std::env::var("PICLOUD_EVENTS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(format!("{data_root}/events")));

        let storage_path = std::env::var("PICLOUD_STORAGE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(format!("{data_root}/storage")));

        let storage_capacity_gb = std::env::var("PICLOUD_STORAGE_CAPACITY_GB")
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| toml.storage_limit_gb())
            .unwrap_or(100);

        let rdf_path = std::env::var("PICLOUD_RDF_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(format!("{data_root}/rdf")));

        let registry_path = std::env::var("PICLOUD_REGISTRY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(format!("{data_root}/registry")));

        Self {
            node_id,
            node_name,
            cluster_domain,
            http_port,
            dns_port,
            raft_port,
            enrollment_port,
            bind_addr,
            events_path,
            storage_path,
            storage_capacity_gb,
            rdf_path,
            registry_path,
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

    let cli = CliArgs::parse();
    let toml_config = PlatformConfig::load(cli.config.as_deref());
    let config = ServerConfig::from_toml_and_env(&toml_config);

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
    let mut cluster = MdnsCluster::start(cluster_config)?;
    info!("Cluster membership started");

    // 2. Start event log (persistent, backed by JSON-lines file)
    //
    // The PersistentEventLog is the LOCAL backing store. It is populated
    // by the Raft apply callback — never written to directly by HTTP
    // handlers or other consumers.
    let event_log = Arc::new(
        PersistentEventLog::open(&config.events_path)
            .expect("failed to open persistent event log"),
    );

    // 2b. Start Raft consensus node
    //
    // The apply callback feeds committed Raft entries into the local event log.
    // This is how replicated events arrive on every node in the cluster.
    let event_log_for_raft = event_log.clone();
    let apply_cb: picloud_cluster::ApplyCallback = Arc::new(move |json: &str| {
        if let Ok(envelope) = serde_json::from_str::<picloud_domain::events::EventEnvelope>(json) {
            let el = event_log_for_raft.clone();
            tokio::spawn(async move {
                if let Err(e) = el.append(envelope).await {
                    error!("Failed to append Raft-committed event: {e}");
                }
            });
        } else {
            warn!("Raft committed entry is not a valid EventEnvelope");
        }
    });

    let raft_node_id = picloud_cluster::node_id_from_uuid(&config.node_id);
    let raft_addr = format!("{}:{}", config.bind_addr, config.http_port);

    // Check if there are already peers discovered via mDNS.
    // If no peers exist, this is the first node — bootstrap a single-node Raft cluster.
    // If peers exist, start without initial members and wait to be added by the leader.
    let peers = cluster.peers().all();
    let initial_members = if peers.is_empty() {
        info!("No peers discovered — bootstrapping as single-node Raft cluster");
        let mut members = std::collections::BTreeMap::new();
        members.insert(raft_node_id, openraft::BasicNode::new(&raft_addr));
        Some(members)
    } else {
        info!(
            peer_count = peers.len(),
            "Peers already discovered — starting Raft without bootstrap (will be added by leader)"
        );
        None
    };

    let raft = picloud_cluster::create_raft_node(
        raft_node_id,
        &raft_addr,
        Some(apply_cb),
        initial_members,
    )
    .await
    .expect("failed to create Raft node");
    let raft = Arc::new(raft);
    cluster.set_raft(Arc::clone(&raft));
    let raft_router = picloud_cluster::raft_rpc_router(Arc::clone(&raft));
    info!(raft_node_id, "Raft consensus node started");

    // 2b-ii. Watch for new peers discovered via mDNS and add them to Raft membership.
    {
        let raft_for_membership = Arc::clone(&raft);
        let peers_for_membership = cluster.peers().clone();
        tokio::spawn(async move {
            // Check for new peers every 5 seconds
            let mut known_raft_members = std::collections::HashSet::new();
            known_raft_members.insert(raft_node_id);
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let current_peers = peers_for_membership.all();
                for peer in &current_peers {
                    let peer_raft_id = picloud_cluster::node_id_from_uuid(&peer.node_id);
                    if known_raft_members.contains(&peer_raft_id) {
                        continue;
                    }
                    let peer_node = openraft::BasicNode::new(&peer.address);
                    info!(
                        peer_raft_id,
                        peer_name = %peer.node_name,
                        peer_addr = %peer.address,
                        "Adding discovered peer to Raft cluster"
                    );
                    match raft_for_membership
                        .add_learner(peer_raft_id, peer_node, true)
                        .await
                    {
                        Ok(_) => {
                            info!(peer_raft_id, "Peer added as Raft learner");
                            known_raft_members.insert(peer_raft_id);
                            // Promote to voter
                            let members: std::collections::BTreeSet<u64> =
                                known_raft_members.iter().copied().collect();
                            if let Err(e) = raft_for_membership
                                .change_membership(members, false)
                                .await
                            {
                                warn!(
                                    peer_raft_id,
                                    error = %e,
                                    "Failed to promote peer to Raft voter"
                                );
                            } else {
                                info!(peer_raft_id, "Peer promoted to Raft voter");
                            }
                        }
                        Err(e) => {
                            warn!(
                                peer_raft_id,
                                error = %e,
                                "Failed to add peer as Raft learner — will retry"
                            );
                        }
                    }
                }
            }
        });
    }

    // 2c. Create the write-through event log that routes all appends through Raft.
    //
    // When any consumer calls event_log_trait.append(event):
    //   1. The event is serialised and submitted via raft.client_write()
    //   2. Raft replicates to all nodes in the cluster
    //   3. Each node's apply callback (above) deserialises and appends to
    //      its local PersistentEventLog
    //   4. The local PersistentEventLog broadcasts to subscribers
    //
    // Reads (subscribe, events_since) go directly to the local backing store.
    let raft_for_write = Arc::clone(&raft);
    let write_cb: picloud_events::WriteCallback = Arc::new(move |event: picloud_domain::events::EventEnvelope| {
        let raft = Arc::clone(&raft_for_write);
        Box::pin(async move {
            let event_json = serde_json::to_string(&event).map_err(|e| {
                picloud_domain::error::PiCloudError::Internal(format!(
                    "failed to serialize event for Raft: {e}"
                ))
            })?;
            let request = picloud_cluster::ClientRequest { event_json };
            raft.client_write(request).await.map_err(|e| {
                picloud_domain::error::PiCloudError::Internal(format!(
                    "Raft client_write failed: {e}"
                ))
            })?;
            Ok(())
        })
    });
    let event_log_local: Arc<dyn EventLog> = event_log.clone();
    let write_through_log = Arc::new(WriteThroughEventLog::new(event_log_local, write_cb));
    let event_log_trait: Arc<dyn EventLog> = write_through_log;

    // Start node failure watcher: subscribe to peer removals from mDNS
    // and emit NodeUnreachable events into the event log.
    {
        let peers = cluster.peers().clone();
        let event_log_for_watcher = event_log_trait.clone();
        let iri_builder = picloud_domain::iri::IriBuilder::new(config.cluster_domain.clone());
        let mut removal_rx = peers.subscribe_removals();
        tokio::spawn(async move {
            info!("Node failure watcher started");
            loop {
                match removal_rx.recv().await {
                    Ok(removed_peer) => {
                        info!(
                            node_id = %removed_peer.node_id,
                            node_name = %removed_peer.node_name,
                            "Peer disappeared from mDNS — emitting NodeUnreachable"
                        );
                        let node_iri = iri_builder.node(&removed_peer.node_name);
                        let event = picloud_domain::events::EventEnvelope::new(
                            iri_builder.event_schema("NodeUnreachable", 1),
                            "NodeUnreachable",
                            node_iri.clone(),
                            None,
                            Uuid::new_v4(),
                            serde_json::json!({
                                "node_id": removed_peer.node_id.to_string(),
                                "node_iri": node_iri.as_str(),
                                "node_name": removed_peer.node_name,
                                "last_address": removed_peer.address,
                            }),
                        );
                        if let Err(e) = event_log_for_watcher.append(event).await {
                            error!("Failed to emit NodeUnreachable event: {e}");
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "Node failure watcher lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("Peer removal channel closed — stopping node failure watcher");
                        break;
                    }
                }
            }
        });
    }

    let cluster: Arc<dyn ClusterMembership> = Arc::new(cluster);
    info!(
        path = %config.events_path.display(),
        events = event_log.len().await,
        "Persistent event log started (writes go through Raft)"
    );

    // 3. Start RDF projector (disk-backed with in-memory fallback)
    //
    // On a fresh node the store is empty and cursor = 0, so it replays
    // the entire event log on first boot.
    //
    // On a restarting node the store already has data and the cursor
    // is restored from a metadata triple — only missed events are replayed.
    let projector = OxigraphProjector::open(
        &config.rdf_path,
        config.cluster_domain.clone(),
    ).unwrap_or_else(|e| {
        warn!("Failed to open disk-backed store ({e}), falling back to in-memory");
        OxigraphProjector::with_domain(config.cluster_domain.clone())
            .expect("in-memory store creation should not fail")
    });
    let projector = Arc::new(projector);

    // Startup catchup: replay any events the projector missed
    {
        let cursor = projector.cursor();
        let missed = event_log.events_since(cursor).await;
        if !missed.is_empty() {
            info!(
                cursor = cursor,
                missed = missed.len(),
                "Replaying missed events into RDF store"
            );
            projector.replay(&missed).await?;
        } else {
            info!(cursor = cursor, "RDF store is up to date");
        }
    }

    // 4. Background projection loop: subscribe to live events and project
    // them as they arrive. This keeps the graph continuously up to date.
    {
        let projector = Arc::clone(&projector);
        let event_log = event_log_trait.clone();
        tokio::spawn(async move {
            let mut rx = match event_log.subscribe(EventFilter::default()).await {
                Ok(rx) => rx,
                Err(e) => {
                    error!("Failed to subscribe to event log for projection: {e}");
                    return;
                }
            };
            info!("Background RDF projection loop started");
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let Err(e) = projector.project(&event).await {
                            error!(
                                event_id = %event.id,
                                event_type = %event.event_type,
                                "Projection failed: {e}"
                            );
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(
                            skipped = n,
                            "Projection subscriber lagged — events may need re-replay"
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("Event log closed — stopping projection loop");
                        break;
                    }
                }
            }
        });
    }

    let projector: Arc<dyn StateProjector> = projector;
    info!(
        path = %config.rdf_path.display(),
        "RDF projector started"
    );

    // 5. Start IAM service
    let iam_key = config.node_id.as_bytes();
    let iam = LocalIdentityProvider::new(iam_key, config.cluster_domain.clone());
    let iam: Arc<dyn IdentityProvider> = Arc::new(iam);
    info!("IAM service started");

    // 5b. Start secret store (uses same key material as IAM for key derivation)
    let secret_store = InMemorySecretStore::new(iam_key);
    let secret_store: Arc<dyn SecretStore> = Arc::new(secret_store);
    info!("Secret store started");

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

    // 6b. Start storage replicator (cross-node file sync)
    {
        let replicator = StorageReplicator::new(
            config.storage_path.clone(),
            cluster.clone(),
            config.node_id,
            10, // sync interval seconds
        );
        replicator.start();
        info!("Storage replicator started (10s interval)");
    }

    // 7. Start workload scheduler (with secret store for env var resolution)
    let scheduler = ProcessScheduler::with_secret_store(
        config.node_id,
        config.cluster_domain.clone(),
        secret_store.clone(),
    );
    let scheduler: Arc<dyn WorkloadScheduler> = Arc::new(scheduler);
    info!("Workload scheduler started");

    // 8. Start network/DNS/TLS
    let dns = InMemoryDnsRegistry::new();
    let dns: Arc<dyn DnsRegistry> = Arc::new(dns);
    let ca = PlatformCa::new()?;

    // Optionally create TLS config for HTTPS serving (controlled by PICLOUD_TLS env var)
    let tls_config = match std::env::var("PICLOUD_TLS")
        .unwrap_or_else(|_| "false".to_string())
        .as_str()
    {
        "true" | "1" | "yes" => {
            match ca.server_tls_config(&config.node_name) {
                Ok(config) => {
                    info!("TLS enabled — HTTPS serving active");
                    Some(config)
                }
                Err(e) => {
                    warn!(error = %e, "Failed to create TLS config — falling back to plain HTTP");
                    None
                }
            }
        }
        _ => {
            info!("TLS disabled (set PICLOUD_TLS=true to enable)");
            None
        }
    };

    info!("Network services started (DNS + CA)");

    // 8b. Start authoritative DNS server (ADR-052)
    let dns_resolver = std::sync::Arc::new(
        picloud_network::dns::resolver::DnsResolver::new(config.cluster_domain.0.clone())
    );
    let dns_cache = dns_resolver.cache();
    {
        let dns_config = picloud_network::dns::server::DnsServerConfig {
            bind_addr: format!("0.0.0.0:{}", config.dns_port),
            domain: config.cluster_domain.0.clone(),
            evict_interval_secs: 60,
        };
        match picloud_network::dns::server::start(dns_config, dns_resolver.clone(), dns_cache.clone()).await {
            Ok(()) => info!(port = config.dns_port, "Authoritative DNS server started"),
            Err(e) => warn!(error = %e, port = config.dns_port, "Failed to start DNS server"),
        }
    }

    // 8c. Initialize Certificate Revocation List (ADR-053)
    let _crl = std::sync::Arc::new(tokio::sync::RwLock::new(
        picloud_network::ca::revocation::RevocationList::new()
    ));

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

    // 10b. Establish cluster identity (ADR-042)
    let identity_store = Arc::new(InMemoryClusterIdentityStore::new());
    {
        let iri_builder = picloud_domain::iri::IriBuilder::new(config.cluster_domain.clone());
        let ca_fingerprint = ca.fingerprint().unwrap_or_else(|_| "unknown".to_string());
        let cluster_id = config.node_id; // Use node ID as cluster ID for single-node bootstrap
        let identity = picloud_domain::resources::ClusterIdentity {
            cluster_id,
            domain: config.cluster_domain.clone(),
            created_at: chrono::Utc::now(),
            ca_fingerprint: ca_fingerprint.clone(),
            enrollment_mode: picloud_domain::identity::EnrollmentMode::default(),
        };

        match identity_store.initialize(identity).await {
            Ok(()) => {
                // Emit ClusterInitialized event
                let init_event = picloud_domain::events::EventEnvelope::new(
                    iri_builder.event_schema("ClusterInitialized", 1),
                    "ClusterInitialized",
                    iri_builder.cluster_identity(),
                    None,
                    Uuid::new_v4(),
                    serde_json::json!({
                        "cluster_id": cluster_id.to_string(),
                        "domain": config.cluster_domain.0,
                        "ca_fingerprint": ca_fingerprint,
                    }),
                );
                event_log_trait.append(init_event).await?;
                info!(
                    cluster_id = %cluster_id,
                    domain = %config.cluster_domain.0,
                    ca_fingerprint = %ca_fingerprint,
                    "Cluster identity established"
                );
            }
            Err(e) => {
                info!("Cluster identity already established: {}", e);
            }
        }
    }
    let _identity_store: Arc<dyn ClusterIdentityStore> = identity_store;

    // 10c. Register built-in alert rules (ADR-041)
    {
        let rules = picloud_domain::resources::builtin_alert_rules();
        info!(
            rule_count = rules.len(),
            "Built-in alert rules registered"
        );
        for rule in &rules {
            info!(
                rule = rule.name,
                alert_type = rule.alert_type,
                threshold = rule.threshold,
                severity = %rule.severity,
                "Registered alert rule"
            );
        }
    }

    // 10d. Start metrics agent (ADR-040)
    {
        let metrics_interval_secs: u64 = std::env::var("PICLOUD_METRICS_INTERVAL")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(15);
        let iri_builder = picloud_domain::iri::IriBuilder::new(config.cluster_domain.clone());
        let agent = MetricsAgent::new(
            event_log_trait.clone(),
            iri_builder,
            node_iri.clone(),
        );
        agent.start(metrics_interval_secs);
        info!(
            interval_secs = metrics_interval_secs,
            "Metrics agent started"
        );
    }

    // 11. Create shared ingress routing table and native ingress router (ADR-048)
    let ingress_table = picloud_http::new_ingress_table();
    let ingress_router: picloud_http::SharedRouter =
        std::sync::Arc::new(picloud_http::IngressRouter::new());
    let tls_state: picloud_http::SharedTls =
        std::sync::Arc::new(picloud_http::TlsState::new());

    // 12. Start resource provisioner (background task)
    {
        let iri_builder = picloud_domain::iri::IriBuilder::new(config.cluster_domain.clone());
        let provisioner = Provisioner::new(
            event_log_trait.clone(),
            projector.clone(),
            storage.clone(),
            scheduler.clone(),
            secret_store.clone(),
            iri_builder,
        )
        .with_ingress_table(ingress_table.clone())
        .with_ingress_router(ingress_router.clone());
        provisioner
            .start()
            .await
            .expect("failed to start resource provisioner");
        info!("Resource provisioner started");
    }

    // 12b. Start inference engine (ADR-038)
    {
        let iri_builder = picloud_domain::iri::IriBuilder::new(config.cluster_domain.clone());
        let engine = Arc::new(InferenceEngine::new(
            projector.clone(),
            event_log_trait.clone(),
            iri_builder,
        ));
        let reconciliation_interval: u64 = std::env::var("PICLOUD_RECONCILIATION_INTERVAL")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(600); // 10 minutes
        engine.start_event_loop();
        engine.start_reconciliation_loop(reconciliation_interval);
        info!(
            reconciliation_interval_secs = reconciliation_interval,
            "Inference engine started"
        );
    }

    // 12c. Start OTel stream and telemetry store (ADR-045, ADR-046)
    let otel_stream = Arc::new(OtelStream::new(4096));
    let telemetry_path = std::env::var("PICLOUD_TELEMETRY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/lib/picloud/telemetry"));
    let telemetry_retention_hours: u64 = std::env::var("PICLOUD_TELEMETRY_RETENTION_HOURS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(168); // 7 days
    let telemetry_store = Arc::new(
        JsonlTelemetryStore::new(&telemetry_path)
            .with_retention_hours(telemetry_retention_hours),
    );
    let telemetry_store_trait: Arc<dyn picloud_domain::traits::TelemetryStore> =
        telemetry_store.clone();

    // Start telemetry retention cleanup
    telemetry_store.start_retention_cleanup();

    // Start OTel aggregator (emits TelemetryAggregated events every 15s)
    {
        let otel_aggregation_interval: u64 = std::env::var("PICLOUD_OTEL_AGGREGATION_INTERVAL")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(15);
        let iri_builder = picloud_domain::iri::IriBuilder::new(config.cluster_domain.clone());
        let aggregator = OtelAggregator::new(
            otel_stream.clone(),
            event_log_trait.clone(),
            telemetry_store_trait.clone(),
            iri_builder,
            node_iri.clone(),
        );
        aggregator.start(otel_aggregation_interval);
        info!(
            interval_secs = otel_aggregation_interval,
            telemetry_path = %telemetry_path.display(),
            retention_hours = telemetry_retention_hours,
            "OTel stream and telemetry store started (ADR-045, ADR-046)"
        );
    }

    // 12d. Create ingress event handler (ADR-048)
    let _ingress_handler = picloud_http::IngressEventHandler::new(
        ingress_router.clone(),
        tls_state,
    );
    // The ingress_handler will be wired into the event subscription loop
    // when the full event stream infrastructure is connected.

    // 13. Start image registry (ADR-054)
    let registry: Arc<dyn picloud_domain::traits::RegistryBackend> = Arc::new(
        LocalRegistryBackend::new(&config.registry_path)
            .expect("failed to initialize image registry"),
    );
    info!(
        path = %config.registry_path.display(),
        "Image registry started"
    );

    // 14. Start HTTP server
    let http_addr = SocketAddr::new(config.bind_addr, config.http_port);
    let mut http_server = PiCloudHttpServer::new(http_addr, config.cluster_domain.clone())
        .with_dependencies(
            event_log_trait.clone(),
            projector.clone(),
            cluster.clone(),
            iam.clone(),
            storage.clone(),
            scheduler.clone(),
        )
        .with_ingress_table(ingress_table)
        .with_ingress_router(ingress_router.clone())
        .with_extra_router(raft_router)
        .with_otel(otel_stream.clone(), telemetry_store_trait.clone())
        .with_storage_path(config.storage_path.clone())
        .with_registry(registry.clone())
        .with_ca(Arc::new(ca) as Arc<dyn picloud_domain::traits::CertificateAuthority>);
    if let Some(tls) = tls_config {
        http_server = http_server.with_tls_config(tls);
    }

    // Emit LeaderElected after a delay to allow Raft to stabilize.
    // Runs in a background task so it doesn't block HTTP server startup.
    {
        let cluster_for_leader = cluster.clone();
        let event_log_for_leader = event_log_trait.clone();
        let node_iri_for_leader = node_iri.clone();
        let domain_for_leader = config.cluster_domain.clone();
        let node_id_str = config.node_id.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            if cluster_for_leader.is_leader().await {
                let iri_builder = picloud_domain::iri::IriBuilder::new(domain_for_leader);
                let leader_event = picloud_domain::events::EventEnvelope::new(
                    iri_builder.event_schema("LeaderElected", 1),
                    "LeaderElected",
                    node_iri_for_leader.clone(),
                    None,
                    Uuid::new_v4(),
                    serde_json::json!({
                        "node_id": node_id_str,
                        "node_iri": node_iri_for_leader.as_str(),
                        "term": 1,
                    }),
                );
                if let Err(e) = event_log_for_leader.append(leader_event).await {
                    warn!("Failed to emit LeaderElected event: {e}");
                } else {
                    info!("LeaderElected event emitted");
                }
            }
        });
    }

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
