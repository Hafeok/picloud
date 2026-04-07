/// PiCloud CLI
///
/// The CLI is the primary management interface for PiCloud (ADR-008).
/// All commands emit events to the cluster and subscribe to the result stream.
/// The CLI never imports slice internals — it only talks HTTP to the cluster.

use clap::{Parser, Subcommand};
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

/// HTTP client for communicating with the PiCloud cluster
struct ClusterClient {
    base_url: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl ClusterClient {
    fn new(domain: &str, token: Option<String>) -> Self {
        Self {
            base_url: format!("https://{}", domain),
            token,
            client: reqwest::Client::builder()
                .danger_accept_invalid_certs(true) // platform CA not yet in trust store
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
        let body = response.json::<serde_json::Value>().await?;
        Ok(body)
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
    let client = ClusterClient::new(&cli.domain, cli.token);

    match cli.command {
        Commands::Cluster { command } => match command {
            ClusterCommands::Init { domain, ca_cert } => {
                println!("Initializing cluster on domain: {}", domain);
                let payload = json!({
                    "domain": domain,
                    "ca_cert": ca_cert,
                });
                match client.post_command("ClusterInit", payload).await {
                    Ok(_) => println!("Cluster initialized successfully"),
                    Err(e) => {
                        // In bootstrap mode, the cluster may not be reachable yet
                        println!("Note: cluster not yet reachable ({})", e);
                        println!("Bootstrap will be completed when the server starts");
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
                            println!("  https://{}/enroll", cli.domain);
                        }
                    }
                    Err(e) => eprintln!("Recovery failed: {}", e),
                }
            }
            ClusterCommands::Status => match client.get("/nodes").await {
                Ok(body) => {
                    println!("Cluster Status");
                    println!("==============");
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&body).unwrap_or_default()
                    );
                }
                Err(e) => eprintln!("Failed to get cluster status: {}", e),
            },
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
                let path = if target.starts_with("https://") {
                    target.clone()
                } else {
                    format!("/products/{}", target)
                };
                match client.get(&path).await {
                    Ok(body) => {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&body).unwrap_or_default()
                        );
                    }
                    Err(e) => eprintln!("Failed to get status: {}", e),
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
                match client.post_command("IdentityCreate", payload).await {
                    Ok(_) => println!("Identity created: {}", name),
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
                            println!("  https://{}/enroll", cli.domain);
                        }
                    }
                    Err(e) => eprintln!("Passkey reset failed: {}", e),
                }
            }
            IdentityCommands::Token => {
                println!("Initiating device authentication flow...");
                println!("Open the following URL in your browser:");
                println!("  https://{}/auth/device", cli.domain);
                println!("Waiting for authentication...");
                // In a real implementation, this would poll for completion
                eprintln!("Device flow not yet implemented — use browser enrollment");
            }
        },
        Commands::Events { command } => match command {
            EventCommands::Stream {
                product,
                correlation_id,
            } => {
                let mut path = "/api/events/stream".to_string();
                let mut params = vec![];
                if let Some(ref p) = product {
                    params.push(format!("product={}", p));
                }
                if let Some(ref c) = correlation_id {
                    params.push(format!("correlation_id={}", c));
                }
                if !params.is_empty() {
                    path = format!("{}?{}", path, params.join("&"));
                }

                println!("Subscribing to event stream...");
                if let Some(ref p) = product {
                    println!("  Product filter: {}", p);
                }
                // In production, this would use SSE streaming
                match client.get(&path).await {
                    Ok(body) => println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default()),
                    Err(e) => eprintln!("Failed to subscribe: {}", e),
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
            SdkCommands::Publish {
                languages,
                registry,
            } => {
                println!("Generating SDKs for: {}", languages.join(", "));
                if let Some(ref r) = registry {
                    println!("  Registry: {}", r);
                }
                let payload = json!({
                    "languages": languages,
                    "registry": registry,
                });
                match client.post_command("SdkPublish", payload).await {
                    Ok(_) => println!("SDKs published successfully"),
                    Err(e) => eprintln!("SDK publish failed: {}", e),
                }
            }
        },
    }

    Ok(())
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
