mod config;
mod harness;
mod invariants;
mod probes;
mod scenarios;
mod upgrade;

use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

use config::ClusterConfig;
use harness::results::RunSummary;
use harness::runner::TestContext;

#[derive(Parser)]
#[command(name = "picloud-test", about = "PiCloud integration & E2E test runner")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run test scenarios against the cluster
    Run {
        /// Filter by suite name (matches ADR or scenario name substring)
        #[arg(long)]
        suite: Option<String>,

        /// Run only the scenario with this exact name
        #[arg(long)]
        scenario: Option<String>,

        /// Path to cluster.toml configuration file
        #[arg(long, default_value = "cluster.toml")]
        config: PathBuf,

        /// Path to the picloud workspace root directory. Scenarios that read
        /// source files (ADR-054, ADR-058) need this to locate docs/, crates/,
        /// and Cargo.toml. Defaults to the compile-time workspace root.
        #[arg(long, env = "PICLOUD_WORKSPACE_ROOT")]
        workspace_root: Option<PathBuf>,
    },

    /// List all registered scenarios
    List,

    /// Rolling upgrade (ADR-055 placeholder)
    Upgrade {
        /// Path to the new picloud-server binary
        #[arg(long)]
        binary: PathBuf,

        /// Path to cluster.toml configuration file
        #[arg(long, default_value = "cluster.toml")]
        config: PathBuf,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Initialize tracing with env filter (RUST_LOG or default)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    match cli.command {
        Command::Run {
            suite,
            scenario,
            config,
            workspace_root,
        } => {
            let cluster_config = match ClusterConfig::load(&config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to load config from {}: {}", config.display(), e);
                    std::process::exit(1);
                }
            };

            // Resolve workspace root: CLI flag > env var > compile-time fallback.
            let ws_root = workspace_root.unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
            });
            let ws_root = ws_root.canonicalize().unwrap_or(ws_root);

            info!(
                domain = cluster_config.cluster.domain.as_str(),
                nodes = cluster_config.nodes.len(),
                workspace_root = ws_root.display().to_string().as_str(),
                "cluster config loaded"
            );

            let ctx = TestContext::new(cluster_config, ws_root);
            let all = scenarios::all_scenarios();

            if all.is_empty() {
                println!("No scenarios registered yet. Add scenarios in crates/picloud-test/src/scenarios/");
                return;
            }

            let start = Instant::now();
            let (passed, failed, skipped) = harness::runner::run_scenarios(
                &all,
                &ctx,
                suite.as_deref(),
                scenario.as_deref(),
            )
            .await;
            let total_duration = start.elapsed();

            let summary = RunSummary::new(passed, failed, skipped, total_duration);
            summary.print();

            if !summary.all_passed() {
                std::process::exit(1);
            }
        }

        Command::List => {
            let all = scenarios::all_scenarios();
            if all.is_empty() {
                println!("No scenarios registered.");
                return;
            }
            println!("{:<40} {}", "SCENARIO", "ADR");
            println!("{}", "-".repeat(55));
            for s in &all {
                println!("{:<40} {}", s.name(), s.adr());
            }
        }

        Command::Upgrade { binary, config } => {
            let cluster_config = match ClusterConfig::load(&config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to load config from {}: {}", config.display(), e);
                    std::process::exit(1);
                }
            };

            info!(
                domain = cluster_config.cluster.domain.as_str(),
                nodes = cluster_config.nodes.len(),
                binary = binary.display().to_string().as_str(),
                "starting rolling upgrade"
            );

            let upgrade_config = upgrade::UpgradeConfig {
                binary_path: binary,
                cluster_config,
            };

            match upgrade::rolling_upgrade(upgrade_config).await {
                Ok(report) => {
                    report.print();
                    if !report.success {
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Upgrade failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}
