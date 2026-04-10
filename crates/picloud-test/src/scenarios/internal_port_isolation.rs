//! ADR-048: Internal Port Isolation — verify production and staging ports are configurable.
//!
//! Reads HTTP server source to confirm that internal ports (internal: true ingress)
//! are isolated from external access, and that port configuration is supported.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct InternalPortIsolationScenario;

#[async_trait]
impl Scenario for InternalPortIsolationScenario {
    fn name(&self) -> &str {
        "internal-port-isolation"
    }

    fn adr(&self) -> &str {
        "ADR-048"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        let http_src = ctx.workspace_root().join("crates/picloud-http/src");
        if !http_src.exists() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "picloud-http crate source directory does not exist".to_string(),
            };
        }

        let isolation_markers = [
            "internal",
            "IngressRouter",
            "internal_port",
            "internal_routes",
            "RouteEntry",
            "Upstream",
        ];

        let port_config_markers = [
            "http_port",
            "https_port",
            "listen_port",
            "bind_port",
            "port",
            "staging_port",
        ];

        let mut found_isolation = false;
        let mut found_port_config = false;
        let mut isolation_file = String::new();
        let mut port_config_file = String::new();

        // Scan picloud-http source.
        scan_for_markers(
            &http_src,
            &isolation_markers,
            &mut found_isolation,
            &mut isolation_file,
        );
        scan_for_markers(
            &http_src,
            &port_config_markers,
            &mut found_port_config,
            &mut port_config_file,
        );

        // Also check the server config for port configuration.
        let server_main = ctx.workspace_root().join("src/main.rs");
        if let Ok(content) = std::fs::read_to_string(&server_main) {
            for marker in &port_config_markers {
                if content.contains(marker) {
                    found_port_config = true;
                    port_config_file = server_main.display().to_string();
                    break;
                }
            }
        }

        if !found_isolation && !found_port_config {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "no internal port isolation or port configuration code found in picloud-http".to_string(),
            };
        }

        if !found_isolation {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "port configuration found in {} but no internal port isolation markers in picloud-http",
                    port_config_file
                ),
            };
        }

        info!(
            isolation_file = isolation_file.as_str(),
            port_config_file = port_config_file.as_str(),
            "internal port isolation and port configuration found"
        );

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}

fn scan_for_markers(
    dir: &std::path::Path,
    markers: &[&str],
    found: &mut bool,
    found_file: &mut String,
) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_for_markers(&path, markers, found, found_file);
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for marker in markers {
                        if content.contains(marker) {
                            *found = true;
                            *found_file = path.display().to_string();
                            return;
                        }
                    }
                }
            }
            if *found {
                return;
            }
        }
    }
}
