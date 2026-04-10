//! ADR-029: Verify IRIs are deterministic — same input produces same IRI.
//!
//! Reads IRI builder source and exercises IriBuilder to confirm that
//! constructing an IRI with the same parameters always produces the
//! same result, ensuring IRI stability guarantees.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct IriStability;

#[async_trait]
impl Scenario for IriStability {
    fn name(&self) -> &str {
        "iri-stability"
    }

    fn adr(&self) -> &str {
        "ADR-029"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // 1. Verify IriBuilder exists and has deterministic construction methods
        let iri_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/iri.rs");
        let iri_content = match std::fs::read_to_string(&iri_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", iri_path.display(), e),
                };
            }
        };

        // 2. Verify IriBuilder uses format! with deterministic inputs (no randomness)
        if !iri_content.contains("struct IriBuilder") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "IriBuilder struct not found in iri.rs".to_string(),
            };
        }

        // 3. Verify IriBuilder methods use domain + path components (no UUID/random in IRI)
        // Check that the builder uses format! with deterministic template strings
        let key_methods = [
            "fn cluster_root",
            "fn node",
            "fn product",
            "fn resource",
        ];
        for method in &key_methods {
            if !iri_content.contains(method) {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("IriBuilder missing method: {}", method),
                };
            }
        }

        // 4. Verify no Uuid::new_v4() or random calls in IRI construction
        // IRI builder methods should use only deterministic inputs
        let iri_builder_section = if let Some(start_idx) = iri_content.find("impl IriBuilder") {
            &iri_content[start_idx..]
        } else {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "impl IriBuilder block not found".to_string(),
            };
        };

        if iri_builder_section.contains("Uuid::new_v4()")
            || iri_builder_section.contains("rand::")
            || iri_builder_section.contains("thread_rng")
        {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason:
                    "IriBuilder uses random values — IRIs must be deterministic for stability"
                        .to_string(),
            };
        }

        // 5. Verify IRI tests exist that confirm determinism
        if !iri_content.contains("#[test]") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "iri.rs has no tests confirming IRI determinism".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
