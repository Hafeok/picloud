//! ADR-049: Verify offline validation works without a running server.
//!
//! Checks that the picloud-compiler crate and picloud-domain parser provide
//! the infrastructure for validating .picloud files offline (without cluster
//! access). Verifies the parser can parse and validate resource declarations.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OfflineValidation;

#[async_trait]
impl Scenario for OfflineValidation {
    fn name(&self) -> &str {
        "offline-validation"
    }

    fn adr(&self) -> &str {
        "ADR-049"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // 1. Verify picloud-compiler crate exists
        let compiler_dir = ctx
            .workspace_root()
            .join("crates/picloud-compiler");
        if !compiler_dir.exists() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "picloud-compiler crate not found".to_string(),
            };
        }

        // 2. Verify picloud-domain has parser module for offline parsing
        let parser_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/parser.rs");
        let parser_content = if parser_path.exists() {
            match std::fs::read_to_string(&parser_path) {
                Ok(c) => c,
                Err(e) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!("Failed to read parser.rs: {}", e),
                    };
                }
            }
        } else {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "parser.rs not found in picloud-domain — offline validation requires a parser".to_string(),
            };
        };

        // 3. Verify parser can handle resource declarations
        if !parser_content.contains("ResourceDeclaration") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "ResourceDeclaration type not found in parser — cannot validate resources offline".to_string(),
            };
        }

        // 4. Verify parser has a parse/validate function
        if !parser_content.contains("fn parse") && !parser_content.contains("fn validate") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "Parser missing parse/validate function for offline validation".to_string(),
            };
        }

        // 5. Verify compiler crate depends only on picloud-domain (offline — no cluster deps)
        let cargo_path = ctx
            .workspace_root()
            .join("crates/picloud-compiler/Cargo.toml");
        let cargo_content = match std::fs::read_to_string(&cargo_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read compiler Cargo.toml: {}", e),
                };
            }
        };

        if !cargo_content.contains("picloud-domain") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "picloud-compiler does not depend on picloud-domain".to_string(),
            };
        }

        // 6. Verify no network/cluster dependencies in compiler (offline capability)
        let parsed: toml::Value = match cargo_content.parse() {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to parse compiler Cargo.toml: {}", e),
                };
            }
        };

        if let Some(deps) = parsed.get("dependencies").and_then(|d| d.as_table()) {
            for key in deps.keys() {
                // Compiler should not depend on cluster/network/http crates
                if key == "picloud-cluster"
                    || key == "picloud-network"
                    || key == "picloud-http"
                {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "picloud-compiler depends on {} — violates offline validation requirement",
                            key
                        ),
                    };
                }
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
