/// PiCloud Server — Composition Root
///
/// This is the only binary that imports all slices.
/// Its only job is to instantiate implementations and wire them together.
/// No business logic lives here — it all lives in slices.
///
/// ADR-034: Vertical slice architecture with stable domain dependency

use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("picloud=info".parse()?)
        )
        .json()
        .init();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "PiCloud server starting"
    );

    // Phase 1 bootstrap order:
    // 1. Load config (cluster domain, CA config)
    // 2. Start mDNS discovery (picloud-cluster)
    // 3. Bootstrap or join Raft cluster (picloud-cluster)
    // 4. Start event log (picloud-events)
    // 5. Start RDF projector (picloud-rdf)
    // 6. Start IAM service (picloud-iam)
    // 7. Start storage backend (picloud-storage)
    // 8. Start workload scheduler (picloud-workload)
    // 9. Start network/DNS/TLS (picloud-network)
    // 10. Start HTTP server — serve IRI space (picloud-http)

    // TODO: wire implementations from each slice
    // Each slice exposes a constructor that returns Arc<dyn Trait>
    // See picloud-domain::traits for the full trait list

    info!("PiCloud server ready");

    // Keep running until signal
    tokio::signal::ctrl_c().await?;
    info!("Shutting down");

    Ok(())
}
