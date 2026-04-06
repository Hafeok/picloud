/// PiCloud CLI
///
/// The CLI is the primary management interface for PiCloud (ADR-008).
/// All commands emit events to the cluster and subscribe to the result stream.
/// The CLI never imports slice internals — it only talks HTTP to the cluster.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "picloud",
    about = "PiCloud — private cloud for Raspberry Pi clusters",
    version
)]
struct Cli {
    /// Cluster domain (default: picloud.local)
    #[arg(long, env = "PICLOUD_DOMAIN", default_value = "picloud.local")]
    domain: String,

    /// Path to identity token
    #[arg(long, env = "PICLOUD_TOKEN")]
    token: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Cluster management
    Cluster {
        #[command(subcommand)]
        command: ClusterCommands,
    },
    /// Resource operations
    Resource {
        #[command(subcommand)]
        command: ResourceCommands,
    },
    /// Identity and access management
    Identity {
        #[command(subcommand)]
        command: IdentityCommands,
    },
    /// Event stream subscription
    Events {
        #[command(subcommand)]
        command: EventCommands,
    },
    /// Graph queries
    Graph {
        #[command(subcommand)]
        command: GraphCommands,
    },
    /// CA management
    Ca {
        #[command(subcommand)]
        command: CaCommands,
    },
    /// SDK generation and publication
    Sdk {
        #[command(subcommand)]
        command: SdkCommands,
    },
}

#[derive(Subcommand)]
enum ClusterCommands {
    /// Bootstrap a new cluster on this node
    Init {
        /// Cluster domain name
        #[arg(long, default_value = "picloud.local")]
        domain: String,
        /// Path to BYO CA certificate (optional — generates one if omitted)
        #[arg(long)]
        ca_cert: Option<String>,
    },
    /// Physical recovery — generate a new bootstrap token from a node
    Recover,
    /// Show cluster status
    Status,
}

#[derive(Subcommand)]
enum ResourceCommands {
    /// Apply all .picloud resource files in a directory
    Apply {
        /// Path to directory containing .picloud files
        path: String,
    },
    /// Delete all resources declared in a directory
    Delete {
        path: String,
    },
    /// Show resource status
    Status {
        /// Product name or resource IRI
        target: String,
    },
}

#[derive(Subcommand)]
enum IdentityCommands {
    /// Create a human identity
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        email: Option<String>,
    },
    /// Initiate passkey reset for a user (admin only)
    ResetPasskey {
        /// Identity name or IRI
        identity: String,
    },
    /// Get a CLI token for the current user (device flow or FIDO2 direct)
    Token,
}

#[derive(Subcommand)]
enum EventCommands {
    /// Subscribe to the platform event stream
    Stream {
        /// Filter to a specific product
        #[arg(long)]
        product: Option<String>,
        /// Filter to a specific correlation ID
        #[arg(long)]
        correlation_id: Option<String>,
    },
}

#[derive(Subcommand)]
enum GraphCommands {
    /// Execute a SPARQL query against the cluster graph
    Query {
        #[arg(long)]
        sparql: String,
        /// Scope to a specific product graph
        #[arg(long)]
        product: Option<String>,
    },
}

#[derive(Subcommand)]
enum CaCommands {
    /// Export the platform CA certificate for client trust store installation
    Export {
        #[arg(long, default_value = "picloud-ca.pem")]
        output: String,
    },
    /// Install the platform CA into the OS trust store
    Install,
}

#[derive(Subcommand)]
enum SdkCommands {
    /// Generate and publish SDKs from the cluster's live ontology
    Publish {
        /// Languages to publish (rust, typescript, dotnet)
        #[arg(long, num_args = 1.., default_values = ["rust", "typescript", "dotnet"])]
        languages: Vec<String>,
        /// Registry override (defaults to crates.io / npm / NuGet)
        #[arg(long)]
        registry: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    // TODO: wire command handlers
    // Each command emits an event to https://{domain}/api/commands
    // and subscribes to the result stream via SSE

    println!("PiCloud CLI — domain: {}", cli.domain);
    Ok(())
}
