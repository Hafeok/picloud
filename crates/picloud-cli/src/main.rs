/// PiCloud CLI
///
/// The CLI is the primary management interface for PiCloud (ADR-008).
/// All commands emit events to the cluster and subscribe to the result stream.
/// The CLI never imports slice internals — it only talks HTTP to the cluster.

#[cfg(feature = "fido2")]
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
#[cfg(feature = "fido2")]
use base64::Engine;
use clap::{Parser, Subcommand};
use futures::StreamExt;
use serde_json::json;
use tracing::{error, info};
use uuid::Uuid;

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

    /// Port to connect to (default: 7443)
    #[arg(long, env = "PICLOUD_PORT", default_value = "7443")]
    port: u16,

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
    /// Tag management (ADR-036)
    Tag {
        #[command(subcommand)]
        command: TagCommands,
    },
    /// Alert management (ADR-041)
    Alerts {
        #[command(subcommand)]
        command: AlertCommands,
    },
    /// Telemetry queries (ADR-045, ADR-046)
    Telemetry {
        #[command(subcommand)]
        command: TelemetryCommands,
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
    /// Replay platform events to rebuild the cluster RDF graph (ADR-035)
    Replay {
        /// Start of the replay time range (RFC 3339)
        #[arg(long)]
        from: String,
        /// End of the replay time range (RFC 3339, default: now)
        #[arg(long)]
        to: Option<String>,
    },
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
    /// Replay product events to rebuild the product's RDF graph (ADR-035)
    Replay {
        /// Product name
        product: String,
        /// Start of the replay time range (RFC 3339)
        #[arg(long)]
        from: String,
        /// End of the replay time range (RFC 3339, default: now)
        #[arg(long)]
        to: Option<String>,
        /// Filter to a specific aggregate type
        #[arg(long)]
        aggregate: Option<String>,
        /// Filter to a specific aggregate ID
        #[arg(long)]
        id: Option<String>,
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
    Token {
        /// Use a physical FIDO2 security key (YubiKey) for authentication
        #[arg(long)]
        fido2: bool,
        /// Identity IRI (required for --fido2)
        #[arg(long)]
        identity: Option<String>,
    },
    /// Register a new passkey for an identity
    Register {
        /// Use a physical FIDO2 security key (YubiKey) for registration
        #[arg(long)]
        fido2: bool,
        /// Identity IRI
        #[arg(long)]
        identity: String,
        /// Display name for the passkey
        #[arg(long)]
        display_name: Option<String>,
    },
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
    /// Generate SDKs from the cluster's live ontology
    Generate {
        /// Languages to generate (rust, typescript, dotnet)
        #[arg(long, num_args = 1.., default_values = ["rust", "typescript", "dotnet"])]
        languages: Vec<String>,
        /// Output directory for generated SDKs
        #[arg(long, default_value = "./generated-sdks")]
        output: String,
    },
    /// Generate and optionally publish SDKs to package registries
    Publish {
        /// Languages to publish (rust, typescript, dotnet)
        #[arg(long, num_args = 1.., default_values = ["rust", "typescript", "dotnet"])]
        languages: Vec<String>,
        /// Registry override (defaults to crates.io / npm / NuGet)
        #[arg(long)]
        registry: Option<String>,
        /// Actually publish (default is dry-run)
        #[arg(long, default_value = "false")]
        publish: bool,
        /// Output directory for generated SDKs
        #[arg(long, default_value = "./generated-sdks")]
        output: String,
    },
}

#[derive(Subcommand)]
enum TagCommands {
    /// Add a tag to a resource
    Add {
        /// Resource path (e.g. photo-app/containers/api-server)
        resource: String,
        /// Tag in key=value format
        tag: String,
    },
    /// Remove a tag from a resource
    Remove {
        /// Resource path (e.g. photo-app/containers/api-server)
        resource: String,
        /// Tag in key=value format
        tag: String,
    },
    /// List all tags on a resource
    List {
        /// Resource path (e.g. photo-app/containers/api-server)
        resource: String,
    },
    /// Find all resources with a specific tag
    Find {
        /// Tag in key=value format
        tag: String,
    },
}

#[derive(Subcommand)]
enum AlertCommands {
    /// List active alerts across the cluster
    List {
        /// Filter by severity (info, warning, critical)
        #[arg(long)]
        severity: Option<String>,
        /// Filter by resource IRI or path
        #[arg(long)]
        resource: Option<String>,
    },
}

#[derive(Subcommand)]
enum TelemetryCommands {
    /// Query telemetry data (traces or metrics)
    Query {
        /// Signal type: "traces" or "metrics"
        #[arg(long, default_value = "traces")]
        signal: String,
        /// Start time (RFC 3339, e.g. "2026-04-07T00:00:00Z")
        #[arg(long)]
        from: Option<String>,
        /// End time (RFC 3339, defaults to now)
        #[arg(long)]
        to: Option<String>,
        /// Filter by service name
        #[arg(long)]
        service: Option<String>,
        /// SQL query (future: parsed server-side; for now, ignored)
        #[arg(long)]
        sql: Option<String>,
    },
}

/// HTTP client for communicating with the PiCloud cluster
struct ClusterClient {
    base_url: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl ClusterClient {
    fn new(domain: &str, port: u16, token: Option<String>) -> Self {
        Self {
            base_url: format!("http://{}:{}", domain, port),
            token,
            client: reqwest::Client::builder()
                .build()
                .expect("failed to create HTTP client"),
        }
    }

    async fn post_command(
        &self,
        command_type: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let correlation_id = Uuid::new_v4();
        let body = json!({
            "command_type": command_type,
            "correlation_id": correlation_id.to_string(),
            "payload": payload,
        });

        info!(
            command_type = command_type,
            correlation_id = %correlation_id,
            "Submitting command"
        );

        let mut request = self
            .client
            .post(format!("{}/api/commands", self.base_url))
            .json(&body);

        if let Some(ref token) = self.token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request.send().await?;
        let status = response.status();
        let body = response.json::<serde_json::Value>().await?;

        if status.is_success() {
            println!("-> Command accepted (correlation_id: {})", correlation_id);
            Ok(body)
        } else {
            error!(status = %status, "Command failed");
            Err(format!("Command failed with status {}: {}", status, body).into())
        }
    }

    async fn post_apply(
        &self,
        resource_file: serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let mut request = self
            .client
            .post(format!("{}/api/apply", self.base_url))
            .json(&resource_file);

        if let Some(ref token) = self.token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request.send().await?;
        let status = response.status();
        let body = response.json::<serde_json::Value>().await?;

        if status.is_success() {
            Ok(body)
        } else {
            Err(format!("Apply failed with status {}: {}", status, body).into())
        }
    }

    async fn post_delete(
        &self,
        product: &str,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let body = json!({ "product": product });

        let mut request = self
            .client
            .post(format!("{}/api/delete", self.base_url))
            .json(&body);

        if let Some(ref token) = self.token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request.send().await?;
        let status = response.status();
        let body = response.json::<serde_json::Value>().await?;

        if status.is_success() {
            Ok(body)
        } else {
            Err(format!("Delete failed with status {}: {}", status, body).into())
        }
    }

    async fn get(&self, path: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let mut request = self
            .client
            .get(format!("{}{}", self.base_url, path))
            .header("Accept", "application/json");

        if let Some(ref token) = self.token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(format!("HTTP {} — {}", status, text).into());
        }
        let body = response.json::<serde_json::Value>().await?;
        Ok(body)
    }

    /// Connect to an SSE endpoint and stream events line-by-line.
    /// Runs until the connection is closed or the caller cancels.
    async fn get_sse_stream(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut request = self
            .client
            .get(format!("{}{}", self.base_url, path))
            .header("Accept", "text/event-stream");

        if let Some(ref token) = self.token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(format!("HTTP {} — {}", status, text).into());
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            // Process complete SSE lines
            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim_end_matches('\r').to_string();
                buffer = buffer[pos + 1..].to_string();

                if line.starts_with("data: ") {
                    let data = &line[6..];
                    // Try to pretty-print JSON, fall back to raw text
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                        let event_type = json
                            .get("event_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let source = json
                            .get("source")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let timestamp = json
                            .get("timestamp")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        println!(
                            "[{}] {} — {}",
                            timestamp, event_type, source
                        );
                        if let Some(payload) = json.get("payload") {
                            println!(
                                "  {}",
                                serde_json::to_string_pretty(payload)
                                    .unwrap_or_default()
                                    .replace('\n', "\n  ")
                            );
                        }
                    } else {
                        println!("{}", data);
                    }
                } else if line.starts_with("event: ") {
                    // SSE event type line — informational
                    let event_name = &line[7..];
                    println!("--- {} ---", event_name);
                }
                // Skip empty lines and comments (lines starting with ':')
            }
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("picloud=info".parse()?),
        )
        .init();

    let cli = Cli::parse();
    let client = ClusterClient::new(&cli.domain, cli.port, cli.token);

    match cli.command {
        Commands::Cluster { command } => match command {
            ClusterCommands::Init { domain, ca_cert } => {
                println!("PiCloud Cluster Bootstrap (ADR-042)");
                println!("===================================");
                println!();

                // Try to fetch existing cluster identity from the server
                match client.get("/cluster/identity").await {
                    Ok(identity) => {
                        // Cluster already initialized — display identity
                        let cluster_id = identity.get("cluster_id").and_then(|v| v.as_str()).unwrap_or("unknown");
                        let id_domain = identity.get("domain").and_then(|v| v.as_str()).unwrap_or(&domain);
                        let fingerprint = identity.get("ca_fingerprint").and_then(|v| v.as_str()).unwrap_or("unknown");
                        let created_at = identity.get("created_at").and_then(|v| v.as_str()).unwrap_or("unknown");

                        println!("Cluster Identity (already initialized):");
                        println!("  Cluster ID:      {}", cluster_id);
                        println!("  Domain:          {}", id_domain);
                        println!("  CA Fingerprint:  {}", fingerprint);
                        println!("  Created:         {}", created_at);
                        println!();
                        println!("This cluster is already initialized. The identity is immutable.");
                    }
                    Err(_) => {
                        // Cluster not yet initialized or server unreachable — display instructions
                        let cluster_id = Uuid::new_v4();
                        println!("Initializing new cluster identity...");
                        println!();
                        println!("Configuration:");
                        println!("  Cluster ID:  {}", cluster_id);
                        println!("  Domain:      {}", domain);
                        if let Some(ref cert) = ca_cert {
                            println!("  CA cert:     {} (bring-your-own)", cert);
                        } else {
                            println!("  CA cert:     auto-generated (platform CA)");
                        }
                        println!();
                        println!("To start the cluster on this node:");
                        println!("  picloud-server");
                        println!("  Environment:");
                        println!("    PICLOUD_DOMAIN={}", domain);
                        if let Some(ref cert) = ca_cert {
                            println!("    PICLOUD_CA_CERT={}", cert);
                        }
                        println!();
                        println!("On additional nodes, run picloud-server with the same domain.");
                        println!("They will discover this cluster via mDNS and join automatically.");
                        println!("Nodes advertising a different domain are invisible (ADR-042).");
                        println!();
                        println!("Once running, check cluster status with:");
                        println!("  picloud --port {} cluster status", cli.port);
                    }
                }
            }
            ClusterCommands::Recover => {
                println!("Initiating physical recovery...");
                println!("This must be run locally on a cluster node.");
                match client.post_command("ClusterRecover", json!({})).await {
                    Ok(resp) => {
                        if let Some(token) = resp.get("bootstrap_token") {
                            println!("Bootstrap token: {}", token);
                            println!("Token expires in 15 minutes. Use it to re-enroll at:");
                            println!("  http://{}:{}/enroll", cli.domain, cli.port);
                        }
                    }
                    Err(e) => eprintln!("Recovery failed: {}", e),
                }
            }
            ClusterCommands::Status => {
                println!("Cluster Status");
                println!("==============");
                println!();

                // Fetch cluster root info
                match client.get("/").await {
                    Ok(info) => {
                        if let Some(domain) = info.get("domain").and_then(|v| v.as_str()) {
                            println!("  Domain:   {}", domain);
                        }
                        if let Some(version) = info.get("version").and_then(|v| v.as_str()) {
                            println!("  Version:  {}", version);
                        }
                        if let Some(leader) = info.get("leader").and_then(|v| v.as_str()) {
                            println!("  Leader:   {}", leader);
                        }
                        if let Some(state) = info.get("state").and_then(|v| v.as_str()) {
                            println!("  State:    {}", state);
                        }
                        println!();
                    }
                    Err(e) => {
                        eprintln!("  (Could not fetch cluster info: {})", e);
                        println!();
                    }
                }

                // Fetch nodes
                match client.get("/nodes").await {
                    Ok(body) => {
                        if let Some(nodes) = body.as_array() {
                            println!("Nodes ({}):", nodes.len());
                            println!("  {:<20} {:<15} {:<10} {}", "NAME", "ADDRESS", "ROLE", "STATUS");
                            println!("  {:<20} {:<15} {:<10} {}", "----", "-------", "----", "------");
                            for node in nodes {
                                let name = node.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                                let addr = node.get("address").and_then(|v| v.as_str()).unwrap_or("?");
                                let role = node.get("role").and_then(|v| v.as_str()).unwrap_or("?");
                                let status = node.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                                println!("  {:<20} {:<15} {:<10} {}", name, addr, role, status);
                            }
                        } else {
                            // Not an array — print raw JSON
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&body).unwrap_or_default()
                            );
                        }
                    }
                    Err(e) => eprintln!("Failed to get node list: {}", e),
                }
            }
            ClusterCommands::Replay { from, to } => {
                println!("Platform Replay (ADR-035)");
                println!("========================");
                println!();
                println!("  From: {}", from);
                if let Some(ref t) = to {
                    println!("  To:   {}", t);
                } else {
                    println!("  To:   (present)");
                }
                println!();

                let mut payload = json!({ "from": from });
                if let Some(ref t) = to {
                    payload["to"] = json!(t);
                }

                match client
                    .client
                    .post(format!("{}/api/replay", client.base_url))
                    .json(&payload)
                    .send()
                    .await
                {
                    Ok(resp) => {
                        let status = resp.status();
                        match resp.json::<serde_json::Value>().await {
                            Ok(body) => {
                                if status.is_success() {
                                    let replay_id = body
                                        .get("replay_id")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown");
                                    println!("Replay started (replay_id: {})", replay_id);
                                    println!("Subscribe to the event stream to monitor progress.");
                                } else {
                                    eprintln!(
                                        "Replay failed: {}",
                                        body.get("error")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("unknown error")
                                    );
                                }
                            }
                            Err(e) => eprintln!("Failed to parse response: {}", e),
                        }
                    }
                    Err(e) => eprintln!("Replay request failed: {}", e),
                }
            }
        },
        Commands::Resource { command } => match command {
            ResourceCommands::Apply { path } => {
                println!("Applying resources from: {}", path);

                // Read all .picloud files from the directory (or single file)
                let file_path = std::path::Path::new(&path);
                let files = if file_path.is_dir() {
                    let mut entries: Vec<_> = std::fs::read_dir(file_path)?
                        .filter_map(|e| e.ok())
                        .filter(|e| {
                            e.path()
                                .extension()
                                .map(|ext| ext == "picloud")
                                .unwrap_or(false)
                        })
                        .map(|e| e.path())
                        .collect();
                    entries.sort();
                    entries
                } else {
                    vec![file_path.to_path_buf()]
                };

                if files.is_empty() {
                    eprintln!("No .picloud files found in {}", path);
                    std::process::exit(1);
                }

                // Parse and merge all resource files
                let mut all_resources = Vec::new();
                for file in &files {
                    let content = std::fs::read_to_string(file)?;
                    let parsed = picloud_domain::parser::ResourceFile::parse(&content)
                        .map_err(|e| format!("{}: {}", file.display(), e))?;
                    all_resources.extend(parsed.resources);
                }

                let resource_file = picloud_domain::parser::ResourceFile {
                    resources: all_resources,
                };

                // Validate
                if let Err(e) = resource_file.validate() {
                    eprintln!("Validation failed: {}", e);
                    std::process::exit(1);
                }

                println!("  Found {} resources in {} file(s)", resource_file.resources.len(), files.len());

                // Submit to cluster
                let payload = serde_json::to_value(&resource_file)?;
                match client.post_apply(payload).await {
                    Ok(resp) => {
                        if let Some(results) = resp.get("results").and_then(|r| r.as_array()) {
                            for result in results {
                                let status = result.get("status").and_then(|s| s.as_str()).unwrap_or("unknown");
                                let name = result.get("name").and_then(|s| s.as_str()).unwrap_or("?");
                                let rtype = result.get("type").and_then(|s| s.as_str()).unwrap_or("?");
                                let symbol = if status == "declared" { "→" } else { "✓" };
                                println!("  {} {} {} ({})", symbol, rtype, name, status);
                            }
                        }
                        println!("Resources applied");
                    }
                    Err(e) => eprintln!("Apply failed: {}", e),
                }
            }
            ResourceCommands::Delete { path } => {
                println!("Deleting resources from: {}", path);

                // Read .picloud files to determine the product name
                let file_path = std::path::Path::new(&path);
                let files = if file_path.is_dir() {
                    let mut entries: Vec<_> = std::fs::read_dir(file_path)
                        .unwrap_or_else(|e| {
                            eprintln!("Failed to read directory: {}", e);
                            std::process::exit(1);
                        })
                        .filter_map(|e| e.ok())
                        .filter(|e| {
                            e.path()
                                .extension()
                                .map(|ext| ext == "picloud")
                                .unwrap_or(false)
                        })
                        .map(|e| e.path())
                        .collect();
                    entries.sort();
                    entries
                } else {
                    vec![file_path.to_path_buf()]
                };

                if files.is_empty() {
                    eprintln!("No .picloud files found in {}", path);
                    std::process::exit(1);
                }

                // Find the product name from the resource files
                let mut product_name = None;
                for file in &files {
                    let content = match std::fs::read_to_string(file) {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("Failed to read {}: {}", file.display(), e);
                            continue;
                        }
                    };
                    let parsed = match picloud_domain::parser::ResourceFile::parse(&content) {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("Failed to parse {}: {}", file.display(), e);
                            continue;
                        }
                    };
                    for decl in &parsed.resources {
                        if let picloud_domain::parser::ResourceDeclaration::Product(p) = decl {
                            product_name = Some(p.name.clone());
                            break;
                        }
                    }
                    if product_name.is_some() {
                        break;
                    }
                }

                let product = match product_name {
                    Some(name) => name,
                    None => {
                        eprintln!("No Product resource found in .picloud files");
                        std::process::exit(1);
                    }
                };

                println!("  Deleting product: {}", product);
                match client.post_delete(&product).await {
                    Ok(resp) => {
                        let correlation_id = resp
                            .get("correlationId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        println!("  Product deletion accepted (correlation_id: {})", correlation_id);
                        println!("  All child resources will be cascading deleted");
                    }
                    Err(e) => eprintln!("Delete failed: {}", e),
                }
            }
            ResourceCommands::Status { target } => {
                let path = if target.starts_with("http://") || target.starts_with("https://") {
                    target.clone()
                } else {
                    format!("/products/{}", target)
                };
                match client.get(&path).await {
                    Ok(body) => {
                        // Show product-level summary if available
                        if let Some(name) = body.get("name").and_then(|v| v.as_str()) {
                            println!("Product: {}", name);
                            if let Some(status) = body.get("status").and_then(|v| v.as_str()) {
                                println!("  Status: {}", status);
                            }
                            if let Some(iri) = body.get("iri").and_then(|v| v.as_str()) {
                                println!("  IRI:    {}", iri);
                            }
                            println!();
                        }

                        // Show child resources if present
                        if let Some(resources) = body.get("resources").and_then(|v| v.as_array()) {
                            println!("Resources ({}):", resources.len());
                            println!("  {:<15} {:<25} {}", "TYPE", "NAME", "STATUS");
                            println!("  {:<15} {:<25} {}", "----", "----", "------");
                            for res in resources {
                                let rtype = res.get("type").and_then(|v| v.as_str()).unwrap_or("?");
                                let rname = res.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                                let rstatus = res.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                                println!("  {:<15} {:<25} {}", rtype, rname, rstatus);
                            }
                        } else {
                            // Fallback: print raw JSON
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&body).unwrap_or_default()
                            );
                        }
                    }
                    Err(e) => eprintln!("Failed to get status: {}", e),
                }
            }
            ResourceCommands::Replay {
                product,
                from,
                to,
                aggregate,
                id,
            } => {
                println!("Product Replay (ADR-035)");
                println!("========================");
                println!();
                println!("  Product: {}", product);
                println!("  From:    {}", from);
                if let Some(ref t) = to {
                    println!("  To:      {}", t);
                } else {
                    println!("  To:      (present)");
                }
                if let Some(ref agg) = aggregate {
                    println!("  Aggregate type: {}", agg);
                }
                if let Some(ref aid) = id {
                    println!("  Aggregate ID:   {}", aid);
                }
                println!();

                let mut payload = json!({ "from": from });
                if let Some(ref t) = to {
                    payload["to"] = json!(t);
                }
                if let Some(ref agg) = aggregate {
                    payload["aggregate_type"] = json!(agg);
                }
                if let Some(ref aid) = id {
                    payload["aggregate_ids"] = json!([aid]);
                }

                // Use "default" as the store name for product-level replay
                let store = "default";
                let url = format!(
                    "{}/products/{}/event-store/{}/replay",
                    client.base_url, product, store
                );
                match client.client.post(&url).json(&payload).send().await {
                    Ok(resp) => {
                        let status = resp.status();
                        match resp.json::<serde_json::Value>().await {
                            Ok(body) => {
                                if status.is_success() {
                                    let replay_id = body
                                        .get("replay_id")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown");
                                    println!("Replay started (replay_id: {})", replay_id);
                                    println!(
                                        "Subscribe to the event stream to monitor progress."
                                    );
                                } else {
                                    eprintln!(
                                        "Replay failed: {}",
                                        body.get("error")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("unknown error")
                                    );
                                }
                            }
                            Err(e) => eprintln!("Failed to parse response: {}", e),
                        }
                    }
                    Err(e) => eprintln!("Replay request failed: {}", e),
                }
            }
        },
        Commands::Identity { command } => match command {
            IdentityCommands::Create { name, email } => {
                println!("Creating identity: {}", name);
                let payload = json!({
                    "name": name,
                    "email": email,
                });
                match client.post_command("IdentityCreated", payload).await {
                    Ok(resp) => {
                        println!("Identity created: {}", name);
                        if let Some(iri) = resp.get("iri").and_then(|v| v.as_str()) {
                            println!("  IRI: {}", iri);
                        }
                        println!();
                        println!("Next steps:");
                        println!("  1. Register a passkey for this identity");
                        println!("  2. Admins must register at least 2 passkeys");
                    }
                    Err(e) => eprintln!("Failed to create identity: {}", e),
                }
            }
            IdentityCommands::ResetPasskey { identity } => {
                println!("Initiating passkey reset for: {}", identity);
                let payload = json!({ "identity": identity });
                match client.post_command("PasskeyReset", payload).await {
                    Ok(resp) => {
                        if let Some(token) = resp.get("enrollment_token") {
                            println!("Enrollment token: {}", token);
                            println!("Complete re-enrollment at:");
                            println!("  http://{}:{}/enroll", cli.domain, cli.port);
                        }
                    }
                    Err(e) => eprintln!("Passkey reset failed: {}", e),
                }
            }
            IdentityCommands::Token { fido2, identity } => {
                if fido2 {
                    #[cfg(feature = "fido2")]
                    {
                        // FIDO2 direct authentication — talk to a physical YubiKey via CTAP2
                        let identity_iri = match identity {
                            Some(ref iri) => iri.clone(),
                            None => {
                                eprintln!("--identity is required with --fido2");
                                eprintln!("Usage: picloud identity token --fido2 --identity <IRI>");
                                std::process::exit(1);
                            }
                        };

                        match fido2_authenticate(&client, &cli.domain, cli.port, &identity_iri).await {
                            Ok(token) => {
                                println!("Authentication successful.");
                                println!();
                                println!("Access token:");
                                println!("  {}", token);
                                println!();
                                println!("Set this token for future CLI calls:");
                                println!("  export PICLOUD_TOKEN={}", token);
                            }
                            Err(e) => eprintln!("FIDO2 authentication failed: {}", e),
                        }
                    }
                    #[cfg(not(feature = "fido2"))]
                    {
                        let _ = identity;
                        eprintln!("FIDO2 support is not enabled in this build.");
                        eprintln!("Rebuild with: cargo build -p picloud-cli --features fido2");
                        std::process::exit(1);
                    }
                } else {
                    // Device flow — browser-based authentication
                    println!("Initiating device authentication flow...");

                    let begin_url = format!("http://{}:{}/auth/device/begin", cli.domain, cli.port);
                    match reqwest::Client::new()
                        .post(&begin_url)
                        .header("Content-Type", "application/json")
                        .body("{}")
                        .send()
                        .await
                    {
                        Ok(resp) => {
                            if !resp.status().is_success() {
                                eprintln!("Failed to start device flow: HTTP {}", resp.status());
                                return Ok(());
                            }
                            match resp.json::<serde_json::Value>().await {
                                Ok(flow) => {
                                    let device_code = flow["device_code"]
                                        .as_str()
                                        .unwrap_or_default()
                                        .to_string();
                                    let verification_url = flow["verification_url"]
                                        .as_str()
                                        .unwrap_or_default()
                                        .to_string();
                                    let interval = flow["interval_secs"].as_u64().unwrap_or(5);
                                    let expires_in = flow["expires_in_secs"].as_u64().unwrap_or(600);

                                    println!();
                                    println!("Open the following URL in your browser to authenticate:");
                                    println!("  {}", verification_url);
                                    println!();
                                    println!("Waiting for authentication (expires in {}s)...", expires_in);

                                    let poll_url = format!(
                                        "http://{}:{}/auth/device/poll",
                                        cli.domain, cli.port
                                    );
                                    let mut attempts = expires_in / interval;
                                    loop {
                                        tokio::time::sleep(
                                            std::time::Duration::from_secs(interval),
                                        )
                                        .await;
                                        attempts = attempts.saturating_sub(1);
                                        if attempts == 0 {
                                            eprintln!("Device flow expired.");
                                            break;
                                        }

                                        let poll_body =
                                            json!({ "device_code": device_code });
                                        match reqwest::Client::new()
                                            .post(&poll_url)
                                            .json(&poll_body)
                                            .send()
                                            .await
                                        {
                                            Ok(poll_resp) => {
                                                if let Ok(result) =
                                                    poll_resp.json::<serde_json::Value>().await
                                                {
                                                    match result["status"].as_str() {
                                                        Some("complete") => {
                                                            let token = result["access_token"]
                                                                .as_str()
                                                                .unwrap_or_default();
                                                            println!("Authentication successful.");
                                                            println!();
                                                            println!("Access token:");
                                                            println!("  {}", token);
                                                            println!();
                                                            println!("Set this token for future CLI calls:");
                                                            println!(
                                                                "  export PICLOUD_TOKEN={}",
                                                                token
                                                            );
                                                            break;
                                                        }
                                                        Some("expired") => {
                                                            eprintln!("Device flow expired.");
                                                            break;
                                                        }
                                                        Some("pending") => {
                                                            eprint!(".");
                                                        }
                                                        _ => {
                                                            eprintln!(
                                                                "Unexpected poll response: {}",
                                                                result
                                                            );
                                                            break;
                                                        }
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                eprintln!("Poll failed: {}", e);
                                                break;
                                            }
                                        }
                                    }
                                }
                                Err(e) => eprintln!("Failed to parse device flow response: {}", e),
                            }
                        }
                        Err(e) => eprintln!("Failed to start device flow: {}", e),
                    }
                }
            }
            IdentityCommands::Register {
                fido2,
                identity,
                display_name,
            } => {
                if fido2 {
                    #[cfg(feature = "fido2")]
                    {
                        match fido2_register(&client, &cli.domain, cli.port, &identity, display_name)
                            .await
                        {
                            Ok(()) => {}
                            Err(e) => eprintln!("FIDO2 registration failed: {}", e),
                        }
                    }
                    #[cfg(not(feature = "fido2"))]
                    {
                        let _ = (identity, display_name);
                        eprintln!("FIDO2 support is not enabled in this build.");
                        eprintln!("Rebuild with: cargo build -p picloud-cli --features fido2");
                        std::process::exit(1);
                    }
                } else {
                    eprintln!("Passkey registration from the CLI currently requires --fido2.");
                    eprintln!("Usage: picloud identity register --fido2 --identity <IRI>");
                }
            }
        },
        Commands::Events { command } => match command {
            EventCommands::Stream {
                product,
                correlation_id,
            } => {
                // Build the SSE endpoint path
                let path = if let Some(ref p) = product {
                    let mut url = format!("/products/{}/events", p);
                    if let Some(ref c) = correlation_id {
                        url = format!("{}?correlation_id={}", url, c);
                    }
                    url
                } else {
                    let mut url = "/api/events/stream".to_string();
                    let mut params = vec![];
                    if let Some(ref c) = correlation_id {
                        params.push(format!("correlation_id={}", c));
                    }
                    if !params.is_empty() {
                        url = format!("{}?{}", url, params.join("&"));
                    }
                    url
                };

                println!("Subscribing to event stream...");
                if let Some(ref p) = product {
                    println!("  Product: {}", p);
                }
                if let Some(ref c) = correlation_id {
                    println!("  Correlation ID: {}", c);
                }
                println!("  (Press Ctrl+C to stop)");
                println!();

                match client.get_sse_stream(&path).await {
                    Ok(()) => println!("Stream ended."),
                    Err(e) => eprintln!("Stream error: {}", e),
                }
            }
        },
        Commands::Graph { command } => match command {
            GraphCommands::Query { sparql, product } => {
                let path = if let Some(ref p) = product {
                    format!(
                        "/products/{}/graph?query={}",
                        p,
                        urlencoding(&sparql)
                    )
                } else {
                    format!("/graph?query={}", urlencoding(&sparql))
                };

                match client.get(&path).await {
                    Ok(body) => {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&body).unwrap_or_default()
                        );
                    }
                    Err(e) => eprintln!("Query failed: {}", e),
                }
            }
        },
        Commands::Ca { command } => match command {
            CaCommands::Export { output } => {
                println!("Exporting CA certificate to: {}", output);
                match client.get("/ca/certificate").await {
                    Ok(body) => {
                        if let Some(pem) = body.get("certificate_pem").and_then(|v| v.as_str()) {
                            std::fs::write(&output, pem)?;
                            println!("CA certificate written to {}", output);
                            println!("Install with:");
                            println!("  picloud ca install");
                        }
                    }
                    Err(e) => eprintln!("Failed to export CA: {}", e),
                }
            }
            CaCommands::Install => {
                println!("Installing CA certificate into OS trust store...");
                // Platform-specific trust store installation
                #[cfg(target_os = "linux")]
                {
                    println!("  cp picloud-ca.pem /usr/local/share/ca-certificates/picloud-ca.crt");
                    println!("  sudo update-ca-certificates");
                }
                #[cfg(target_os = "macos")]
                {
                    println!(
                        "  sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain picloud-ca.pem"
                    );
                }
                println!("Run the commands above to complete installation.");
            }
        },
        Commands::Sdk { command } => match command {
            SdkCommands::Generate {
                languages,
                output,
            } => {
                let output_dir = std::path::PathBuf::from(&output);
                let domain = picloud_domain::iri::ClusterDomain(cli.domain.clone());
                let generator = picloud_sdk_gen::SdkGenerator::new(domain, output_dir);

                for lang_str in &languages {
                    let language = match parse_sdk_language(lang_str) {
                        Some(l) => l,
                        None => {
                            eprintln!("Unknown SDK language: {}", lang_str);
                            continue;
                        }
                    };
                    match generator.generate(language) {
                        Ok(result) => {
                            println!("Generated {} SDK at {}", language, result.output_path.display());
                            for f in &result.files_generated {
                                println!("  {}", f);
                            }
                        }
                        Err(e) => eprintln!("Failed to generate {} SDK: {}", lang_str, e),
                    }
                }
            }
            SdkCommands::Publish {
                languages,
                registry,
                publish,
                output,
            } => {
                let dry_run = !publish;
                let output_dir = std::path::PathBuf::from(&output);
                let domain = picloud_domain::iri::ClusterDomain(cli.domain.clone());
                let generator = picloud_sdk_gen::SdkGenerator::new(domain, output_dir);

                if dry_run {
                    println!("Dry-run mode (use --publish to actually publish)");
                }

                for lang_str in &languages {
                    let language = match parse_sdk_language(lang_str) {
                        Some(l) => l,
                        None => {
                            eprintln!("Unknown SDK language: {}", lang_str);
                            continue;
                        }
                    };
                    match generator.publish(language, dry_run, registry.as_deref()) {
                        Ok(result) => {
                            println!(
                                "{} {} SDK at {}",
                                if dry_run { "Would publish" } else { "Published" },
                                result.language,
                                result.output_path.display(),
                            );
                            println!("  Command: {}", result.command);
                        }
                        Err(e) => eprintln!("Failed to publish {} SDK: {}", lang_str, e),
                    }
                }
            }
        },
        Commands::Tag { command } => match command {
            TagCommands::Add { resource, tag } => {
                let (key, value) = match tag.split_once('=') {
                    Some((k, v)) => (k.to_string(), v.to_string()),
                    None => {
                        eprintln!("Tag must be in key=value format");
                        std::process::exit(1);
                    }
                };

                // Resolve resource path to IRI
                let resource_iri = if resource.starts_with("http://") || resource.starts_with("https://") {
                    resource.clone()
                } else {
                    // Parse as product/resource_type/name
                    let parts: Vec<&str> = resource.split('/').collect();
                    if parts.len() == 3 {
                        format!("https://{}/products/{}/{}/{}", cli.domain, parts[0], parts[1], parts[2])
                    } else if parts.len() == 1 {
                        format!("https://{}/products/{}", cli.domain, parts[0])
                    } else {
                        eprintln!("Resource path must be product/type/name or a full IRI");
                        std::process::exit(1);
                    }
                };

                let payload = json!({
                    "resource_iri": resource_iri,
                    "key": key,
                    "value": value,
                });

                let mut request = client
                    .client
                    .post(format!("{}/api/tags/add", client.base_url))
                    .json(&payload);
                if let Some(ref token) = client.token {
                    request = request.header("Authorization", format!("Bearer {}", token));
                }

                match request.send().await {
                    Ok(resp) if resp.status().is_success() => {
                        println!("Tag added: {}={} on {}", key, value, resource);
                    }
                    Ok(resp) => {
                        let text = resp.text().await.unwrap_or_default();
                        eprintln!("Failed to add tag: {}", text);
                    }
                    Err(e) => eprintln!("Failed to add tag: {}", e),
                }
            }
            TagCommands::Remove { resource, tag } => {
                let (key, value) = match tag.split_once('=') {
                    Some((k, v)) => (k.to_string(), v.to_string()),
                    None => {
                        eprintln!("Tag must be in key=value format");
                        std::process::exit(1);
                    }
                };

                let resource_iri = if resource.starts_with("http://") || resource.starts_with("https://") {
                    resource.clone()
                } else {
                    let parts: Vec<&str> = resource.split('/').collect();
                    if parts.len() == 3 {
                        format!("https://{}/products/{}/{}/{}", cli.domain, parts[0], parts[1], parts[2])
                    } else if parts.len() == 1 {
                        format!("https://{}/products/{}", cli.domain, parts[0])
                    } else {
                        eprintln!("Resource path must be product/type/name or a full IRI");
                        std::process::exit(1);
                    }
                };

                let payload = json!({
                    "resource_iri": resource_iri,
                    "key": key,
                    "value": value,
                });

                let mut request = client
                    .client
                    .post(format!("{}/api/tags/remove", client.base_url))
                    .json(&payload);
                if let Some(ref token) = client.token {
                    request = request.header("Authorization", format!("Bearer {}", token));
                }

                match request.send().await {
                    Ok(resp) if resp.status().is_success() => {
                        println!("Tag removed: {}={} from {}", key, value, resource);
                    }
                    Ok(resp) => {
                        let text = resp.text().await.unwrap_or_default();
                        eprintln!("Failed to remove tag: {}", text);
                    }
                    Err(e) => eprintln!("Failed to remove tag: {}", e),
                }
            }
            TagCommands::List { resource } => {
                let resource_iri = if resource.starts_with("http://") || resource.starts_with("https://") {
                    resource.clone()
                } else {
                    let parts: Vec<&str> = resource.split('/').collect();
                    if parts.len() == 3 {
                        format!("https://{}/products/{}/{}/{}", cli.domain, parts[0], parts[1], parts[2])
                    } else if parts.len() == 1 {
                        format!("https://{}/products/{}", cli.domain, parts[0])
                    } else {
                        eprintln!("Resource path must be product/type/name or a full IRI");
                        std::process::exit(1);
                    }
                };

                let path = format!("/api/tags?resource={}", urlencoding(&resource_iri));
                match client.get(&path).await {
                    Ok(body) => {
                        if let Some(tags) = body.get("tags").and_then(|v| v.as_array()) {
                            if tags.is_empty() {
                                println!("No tags on {}", resource);
                            } else {
                                println!("Tags on {}:", resource);
                                for tag in tags {
                                    let k = tag.get("key").and_then(|v| v.as_str()).unwrap_or("?");
                                    let v = tag.get("value").and_then(|v| v.as_str()).unwrap_or("?");
                                    println!("  {}={}", k, v);
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("Failed to list tags: {}", e),
                }
            }
            TagCommands::Find { tag } => {
                let (key, value) = match tag.split_once('=') {
                    Some((k, v)) => (k.to_string(), v.to_string()),
                    None => {
                        eprintln!("Tag must be in key=value format");
                        std::process::exit(1);
                    }
                };

                let path = format!("/api/tags/find?key={}&value={}", urlencoding(&key), urlencoding(&value));
                match client.get(&path).await {
                    Ok(body) => {
                        if let Some(resources) = body.get("resources").and_then(|v| v.as_array()) {
                            if resources.is_empty() {
                                println!("No resources found with tag {}={}", key, value);
                            } else {
                                println!("Resources with tag {}={}:", key, value);
                                for res in resources {
                                    let iri = res.get("@id").and_then(|v| v.as_str()).unwrap_or("?");
                                    println!("  {}", iri);
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("Failed to find resources: {}", e),
                }
            }
        },
        Commands::Telemetry { command } => match command {
            TelemetryCommands::Query {
                signal,
                from,
                to,
                service,
                sql,
            } => {
                if sql.is_some() {
                    println!("Note: SQL parsing is not yet implemented. Returning all matching records.");
                }

                let endpoint = match signal.as_str() {
                    "traces" | "spans" => "/telemetry/spans",
                    "metrics" => "/telemetry/metrics",
                    _ => {
                        eprintln!("Unknown signal type: {}. Use 'traces' or 'metrics'.", signal);
                        std::process::exit(1);
                    }
                };

                let mut params = Vec::new();
                if let Some(ref f) = from {
                    params.push(format!("from={}", urlencoding(f)));
                }
                if let Some(ref t) = to {
                    params.push(format!("to={}", urlencoding(t)));
                }
                if let Some(ref s) = service {
                    params.push(format!("service={}", urlencoding(s)));
                }

                let path = if params.is_empty() {
                    endpoint.to_string()
                } else {
                    format!("{}?{}", endpoint, params.join("&"))
                };

                match client.get(&path).await {
                    Ok(body) => {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&body).unwrap_or_default()
                        );
                    }
                    Err(e) => eprintln!("Telemetry query failed: {}", e),
                }
            }
        },
        Commands::Alerts { command } => match command {
            AlertCommands::List { severity, resource } => {
                // Query the graph for active picloud:Alert triples (ADR-041)
                let sparql = r#"
                    SELECT ?resource ?type ?severity ?message ?timestamp
                    WHERE {
                        ?alert a <https://picloud.local/ontology#Alert> ;
                               <https://picloud.local/ontology#alertResource> ?resource ;
                               <https://picloud.local/ontology#alertType> ?type ;
                               <https://picloud.local/ontology#alertSeverity> ?severity ;
                               <https://picloud.local/ontology#alertMessage> ?message ;
                               <https://picloud.local/ontology#alertTimestamp> ?timestamp .
                    }
                    ORDER BY DESC(?timestamp)
                "#;

                let path = format!("/graph?query={}", urlencoding(sparql));
                match client.get(&path).await {
                    Ok(body) => {
                        if let Some(bindings) = body.get("bindings").and_then(|v| v.as_array()) {
                            // Filter by severity if specified
                            let filtered: Vec<&serde_json::Value> = bindings
                                .iter()
                                .filter(|b| {
                                    if let Some(ref sev) = severity {
                                        b.get("severity")
                                            .and_then(|v| v.as_str())
                                            .map(|s| s == sev.as_str())
                                            .unwrap_or(false)
                                    } else {
                                        true
                                    }
                                })
                                .filter(|b| {
                                    if let Some(ref res) = resource {
                                        b.get("resource")
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.contains(res.as_str()))
                                            .unwrap_or(false)
                                    } else {
                                        true
                                    }
                                })
                                .collect();

                            if filtered.is_empty() {
                                println!("No active alerts.");
                            } else {
                                println!("Active Alerts ({}):", filtered.len());
                                println!("  {:<10} {:<25} {:<40} {}", "SEVERITY", "TYPE", "RESOURCE", "MESSAGE");
                                println!("  {:<10} {:<25} {:<40} {}", "--------", "----", "--------", "-------");
                                for alert in &filtered {
                                    let sev = alert.get("severity").and_then(|v| v.as_str()).unwrap_or("?");
                                    let atype = alert.get("type").and_then(|v| v.as_str()).unwrap_or("?");
                                    let res = alert.get("resource").and_then(|v| v.as_str()).unwrap_or("?");
                                    let msg = alert.get("message").and_then(|v| v.as_str()).unwrap_or("?");
                                    println!("  {:<10} {:<25} {:<40} {}", sev, atype, res, msg);
                                }
                            }
                        } else {
                            // Fallback: try /api/alerts endpoint
                            match client.get("/api/alerts").await {
                                Ok(alerts_body) => {
                                    println!(
                                        "{}",
                                        serde_json::to_string_pretty(&alerts_body).unwrap_or_default()
                                    );
                                }
                                Err(e) => eprintln!("Failed to query alerts: {}", e),
                            }
                        }
                    }
                    Err(e) => eprintln!("Failed to query alerts: {}", e),
                }
            }
        },
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// FIDO2 direct authentication — CTAP2 HID with a physical security key
// ---------------------------------------------------------------------------

#[cfg(feature = "fido2")]
/// Authenticate using a physical FIDO2 security key (YubiKey, etc.).
///
/// Flow:
///   1. POST /auth/login/begin  -> get challenge + allowed credential IDs
///   2. CTAP2 get_assertion()   -> YubiKey signs the challenge (user touch)
///   3. POST /auth/login/complete -> server verifies ECDSA, returns token
async fn fido2_authenticate(
    _client: &ClusterClient,
    domain: &str,
    port: u16,
    identity_iri: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    use ctap_hid_fido2::fidokey::GetAssertionArgsBuilder;

    println!("FIDO2 Authentication");
    println!("====================");
    println!("Identity: {}", identity_iri);
    println!();

    // Step 1: Begin authentication — get challenge from server
    let begin_url = format!("http://{}:{}/auth/login/begin", domain, port);
    let begin_body = json!({ "identity_iri": identity_iri });
    let resp = reqwest::Client::new()
        .post(&begin_url)
        .json(&begin_body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Server rejected login begin: {}", text).into());
    }

    let begin_resp: serde_json::Value = resp.json().await?;
    let challenge_id = begin_resp["challenge_id"]
        .as_str()
        .ok_or("missing challenge_id in response")?
        .to_string();
    let challenge_b64 = begin_resp["options"]["challenge"]
        .as_str()
        .or_else(|| begin_resp["challenge"].as_str())
        .ok_or("missing challenge in response")?;
    let rp_id = begin_resp["options"]["rp_id"]
        .as_str()
        .or_else(|| begin_resp["rp_id"].as_str())
        .unwrap_or("picloud.local");

    let challenge_bytes = URL_SAFE_NO_PAD.decode(challenge_b64)?;

    // Collect allowed credential IDs
    let allow_credentials: Vec<Vec<u8>> = begin_resp["allow_credentials"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter_map(|s| URL_SAFE_NO_PAD.decode(s).ok())
                .collect()
        })
        .unwrap_or_default();

    // Step 2: Talk to the physical FIDO2 device via CTAP2 HID
    println!("Looking for FIDO2 security keys...");
    let devices = ctap_hid_fido2::get_fidokey_devices();
    if devices.is_empty() {
        return Err(
            "No FIDO2 security keys found.\n\
             Make sure your YubiKey is plugged in.\n\
             On Linux, you may need udev rules for HID access:\n\n\
             Create /etc/udev/rules.d/70-fido2.rules with:\n  \
               KERNEL==\"hidraw*\", SUBSYSTEM==\"hidraw\", \
               ATTRS{idVendor}==\"1050\", MODE=\"0660\", GROUP=\"plugdev\"\n\n\
             Then run: sudo udevadm control --reload-rules && sudo udevadm trigger"
                .into(),
        );
    }

    let device_info = &devices[0];
    println!("  Found: {}", device_info.product_string);
    println!();

    // Build client data JSON (WebAuthn spec)
    let client_data_json = serde_json::to_vec(&json!({
        "type": "webauthn.get",
        "challenge": challenge_b64,
        "origin": format!("https://{}", rp_id),
        "crossOrigin": false,
    }))?;

    println!("Touch your security key to authenticate...");

    let device = ctap_hid_fido2::FidoKeyHid::new(
        &[device_info.param.clone()],
        &ctap_hid_fido2::Cfg::init(),
    )
    .map_err(|e| format!("Failed to open FIDO2 device: {e}"))?;

    let mut args_builder = GetAssertionArgsBuilder::new(rp_id, &challenge_bytes);

    // Add allowed credential IDs
    for cred_id in &allow_credentials {
        args_builder = args_builder.add_credential_id(cred_id);
    }

    let args = args_builder.build();
    let assertions = device
        .get_assertion_with_args(&args)
        .map_err(|e| {
            format!(
                "FIDO2 get_assertion failed: {e}\n\
                 Make sure the correct key is connected and has a credential for this identity."
            )
        })?;

    if assertions.is_empty() {
        return Err("No assertions returned from security key.".into());
    }

    let assertion = &assertions[0];

    // Step 3: Send the authenticator response to the server
    let complete_url = format!("http://{}:{}/auth/login/complete", domain, port);
    let complete_body = json!({
        "challenge_id": challenge_id,
        "credential_id": URL_SAFE_NO_PAD.encode(&assertion.credential_id),
        "signature": URL_SAFE_NO_PAD.encode(&assertion.signature),
        "authenticator_data": URL_SAFE_NO_PAD.encode(&assertion.auth_data),
        "client_data_json": URL_SAFE_NO_PAD.encode(&client_data_json),
        "signature_format": "webauthn",
    });

    let resp = reqwest::Client::new()
        .post(&complete_url)
        .json(&complete_body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Server rejected authentication: {}", text).into());
    }

    let result: serde_json::Value = resp.json().await?;
    let token = result["access_token"]
        .as_str()
        .ok_or("missing access_token in response")?
        .to_string();

    Ok(token)
}

#[cfg(feature = "fido2")]
/// Register a new FIDO2 passkey on a physical security key.
///
/// Flow:
///   1. POST /auth/register/begin    -> get challenge + RP info
///   2. CTAP2 make_credential()      -> YubiKey creates credential (user touch)
///   3. POST /auth/register/complete  -> server stores the public key
async fn fido2_register(
    _client: &ClusterClient,
    domain: &str,
    port: u16,
    identity_iri: &str,
    display_name: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    use ctap_hid_fido2::fidokey::MakeCredentialArgsBuilder;

    println!("FIDO2 Passkey Registration");
    println!("==========================");
    println!("Identity: {}", identity_iri);
    println!();

    // Step 1: Begin registration — get challenge from server
    let begin_url = format!("http://{}:{}/auth/register/begin", domain, port);
    let begin_body = json!({ "identity_iri": identity_iri });
    let resp = reqwest::Client::new()
        .post(&begin_url)
        .json(&begin_body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Server rejected registration begin: {}", text).into());
    }

    let begin_resp: serde_json::Value = resp.json().await?;
    let challenge_id = begin_resp["challenge_id"]
        .as_str()
        .ok_or("missing challenge_id in response")?
        .to_string();
    let challenge_b64 = begin_resp["options"]["challenge"]
        .as_str()
        .or_else(|| begin_resp["challenge"].as_str())
        .ok_or("missing challenge in response")?;
    let rp_id = begin_resp["options"]["rp_id"]
        .as_str()
        .or_else(|| begin_resp["rp_id"].as_str())
        .unwrap_or("picloud.local");

    let challenge_bytes = URL_SAFE_NO_PAD.decode(challenge_b64)?;

    // Step 2: Talk to the physical FIDO2 device
    println!("Looking for FIDO2 security keys...");
    let devices = ctap_hid_fido2::get_fidokey_devices();
    if devices.is_empty() {
        return Err(
            "No FIDO2 security keys found.\n\
             Make sure your YubiKey is plugged in.\n\
             On Linux, you may need udev rules for HID access:\n\n\
             Create /etc/udev/rules.d/70-fido2.rules with:\n  \
               KERNEL==\"hidraw*\", SUBSYSTEM==\"hidraw\", \
               ATTRS{idVendor}==\"1050\", MODE=\"0660\", GROUP=\"plugdev\"\n\n\
             Then run: sudo udevadm control --reload-rules && sudo udevadm trigger"
                .into(),
        );
    }

    let device_info = &devices[0];
    println!("  Found: {}", device_info.product_string);
    println!();
    println!("Touch your security key to create a new credential...");

    let device = ctap_hid_fido2::FidoKeyHid::new(
        &[device_info.param.clone()],
        &ctap_hid_fido2::Cfg::init(),
    )
    .map_err(|e| format!("Failed to open FIDO2 device: {e}"))?;

    let args = MakeCredentialArgsBuilder::new(rp_id, &challenge_bytes).build();

    let attestation = device.make_credential_with_args(&args).map_err(|e| {
        format!(
            "FIDO2 make_credential failed: {e}\n\
             Make sure the correct key is connected and supports FIDO2."
        )
    })?;

    println!("Credential created on security key.");

    // Extract the credential ID and public key from the attestation.
    let credential_id = &attestation.credential_descriptor.id;
    let credential_id_b64 = URL_SAFE_NO_PAD.encode(credential_id);

    // The public key is available as DER-encoded SubjectPublicKeyInfo.
    // For P-256, the DER contains the 65-byte uncompressed EC point (0x04 || x || y)
    // as the last 65 bytes of the encoding.
    let der = &attestation.credential_publickey.der;
    let public_key_bytes = if der.len() >= 65 {
        // Extract the trailing uncompressed point from the SubjectPublicKeyInfo DER
        let offset = der.len() - 65;
        if der[offset] == 0x04 {
            der[offset..].to_vec()
        } else {
            // Fall back to using the full DER
            der.clone()
        }
    } else {
        der.clone()
    };

    let public_key_b64 = URL_SAFE_NO_PAD.encode(&public_key_bytes);

    // Extract AAGUID if available
    let aaguid_str = if !attestation.aaguid.is_empty() {
        Some(URL_SAFE_NO_PAD.encode(&attestation.aaguid))
    } else {
        None
    };

    // Step 3: Send the credential to the server
    let complete_url = format!("http://{}:{}/auth/register/complete", domain, port);
    let complete_body = json!({
        "challenge_id": challenge_id,
        "credential_id": credential_id_b64,
        "public_key": public_key_b64,
        "aaguid": aaguid_str,
        "display_name": display_name.as_deref().unwrap_or("FIDO2 Security Key"),
    });

    let resp = reqwest::Client::new()
        .post(&complete_url)
        .json(&complete_body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Server rejected registration: {}", text).into());
    }

    println!();
    println!("Passkey registered successfully.");
    println!("  Credential ID: {}", credential_id_b64);
    if let Some(ref name) = display_name {
        println!("  Display name:  {}", name);
    }
    println!();
    println!("You can now authenticate with:");
    println!("  picloud identity token --fido2 --identity {}", identity_iri);

    Ok(())
}

/// Parse a language string into an SdkLanguage enum.
fn parse_sdk_language(s: &str) -> Option<picloud_sdk_gen::SdkLanguage> {
    match s.to_lowercase().as_str() {
        "rust" => Some(picloud_sdk_gen::SdkLanguage::Rust),
        "typescript" | "ts" => Some(picloud_sdk_gen::SdkLanguage::TypeScript),
        "dotnet" | ".net" | "csharp" => Some(picloud_sdk_gen::SdkLanguage::DotNet),
        _ => None,
    }
}

/// Simple URL encoding for query parameters
fn urlencoding(s: &str) -> String {
    s.replace(' ', "%20")
        .replace('#', "%23")
        .replace('&', "%26")
        .replace('?', "%3F")
        .replace('{', "%7B")
        .replace('}', "%7D")
}
