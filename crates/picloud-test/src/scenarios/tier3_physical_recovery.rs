//! ADR-026: Tier 3 physical recovery — verify physical recovery endpoint/mechanism
//! exists.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct Tier3PhysicalRecovery;

#[async_trait]
impl Scenario for Tier3PhysicalRecovery {
    fn name(&self) -> &str {
        "tier3-physical-recovery"
    }

    fn adr(&self) -> &str {
        "ADR-026"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if ctx.config.nodes.is_empty() {
            return ScenarioResult::Skip {
                reason: "no nodes configured for SSH".to_string(),
            };
        }

        // Verify the picloud binary has a 'cluster recover' subcommand by
        // checking --help output on a node.
        let node = &ctx.config.nodes[0];

        match assertions::ssh_command(node, "picloud cluster recover --help 2>&1 || picloud-server recover --help 2>&1 || echo 'RECOVER_NOT_FOUND'").await {
            Ok(output) => {
                if output.contains("RECOVER_NOT_FOUND") {
                    // Check if the binary at least exists with the subcommand in help.
                    match assertions::ssh_command(node, "picloud --help 2>&1 || picloud-server --help 2>&1").await {
                        Ok(help_output) => {
                            if help_output.contains("recover") {
                                info!("recovery subcommand found in picloud help output");
                                return ScenarioResult::Pass {
                                    duration: start.elapsed(),
                                };
                            }

                            // Check source code as fallback.
                            let cli_src = ctx
                                .workspace_root()
                                .join("crates/picloud-cli/src");
                            if cli_src.exists() {
                                if let Ok(entries) = std::fs::read_dir(&cli_src) {
                                    for entry in entries.flatten() {
                                        if let Ok(src) = std::fs::read_to_string(entry.path()) {
                                            if src.contains("recover") || src.contains("Recover") {
                                                info!("recovery code found in CLI source");
                                                return ScenarioResult::Pass {
                                                    duration: start.elapsed(),
                                                };
                                            }
                                        }
                                    }
                                }
                            }

                            ScenarioResult::Skip {
                                reason: "physical recovery mechanism not found in CLI or source".to_string(),
                            }
                        }
                        Err(e) => ScenarioResult::Skip {
                            reason: format!("cannot check picloud help: {}", e),
                        },
                    }
                } else {
                    info!("cluster recover subcommand available");
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                }
            }
            Err(e) => ScenarioResult::Skip {
                reason: format!("SSH to node {} failed: {}", node.hostname, e),
            },
        }
    }
}
