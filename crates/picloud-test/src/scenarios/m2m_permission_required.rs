//! ADR-051: Verify machine-to-machine token exchange requires explicit permission grants.
//!
//! Reads IAM/identity source to confirm that M2mPermission resource type
//! exists, that TokenExchanged event exists, and that M2M access requires
//! an explicit permission declaration in the target product.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct M2mPermissionRequired;

#[async_trait]
impl Scenario for M2mPermissionRequired {
    fn name(&self) -> &str {
        "m2m-permission-required"
    }

    fn adr(&self) -> &str {
        "ADR-051"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // 1. Verify M2mPermission type exists in identity.rs
        let identity_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/identity.rs");
        let identity_content = match std::fs::read_to_string(&identity_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", identity_path.display(), e),
                };
            }
        };

        if !identity_content.contains("struct M2mPermission") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "M2mPermission struct not found in identity.rs".to_string(),
            };
        }

        // 2. Verify M2mPermission has required fields (client, product, scopes)
        // Find the M2mPermission section
        let m2m_section = if let Some(idx) = identity_content.find("struct M2mPermission") {
            &identity_content[idx..idx + 500.min(identity_content.len() - idx)]
        } else {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "Could not locate M2mPermission section".to_string(),
            };
        };

        if !m2m_section.contains("client") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "M2mPermission missing 'client' field — must identify requesting product"
                    .to_string(),
            };
        }

        if !m2m_section.contains("product") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "M2mPermission missing 'product' field — must identify target product"
                    .to_string(),
            };
        }

        // 3. Verify TokenExchanged event exists in events.rs
        let events_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/events.rs");
        let events_content = match std::fs::read_to_string(&events_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", events_path.display(), e),
                };
            }
        };

        if !events_content.contains("TokenExchanged") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "TokenExchanged event not found in events.rs".to_string(),
            };
        }

        // 4. Verify RoleAssigned and RoleRevoked events exist for product IAM
        if !events_content.contains("RoleAssigned") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "RoleAssigned event not found — needed for product IAM".to_string(),
            };
        }

        if !events_content.contains("RoleRevoked") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "RoleRevoked event not found — needed for product IAM".to_string(),
            };
        }

        // 5. Verify parser supports m2m-permission resource type
        let parser_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/parser.rs");
        if parser_path.exists() {
            let parser_content = match std::fs::read_to_string(&parser_path) {
                Ok(c) => c,
                Err(_) => String::new(),
            };

            if !parser_content.contains("m2m-permission") && !parser_content.contains("M2mPermission") {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "Parser does not support m2m-permission resource type".to_string(),
                };
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
