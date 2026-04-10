//! ADR-033: Verify SDK generator types/templates exist for Rust, TypeScript, .NET.
//!
//! Reads picloud-sdk-gen source to confirm that the SdkGenerator type exists,
//! supports all three language targets, and has generate/publish methods.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct SdkGeneration;

#[async_trait]
impl Scenario for SdkGeneration {
    fn name(&self) -> &str {
        "sdk-generation"
    }

    fn adr(&self) -> &str {
        "ADR-033"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // 1. Verify picloud-sdk-gen crate exists
        let sdk_gen_dir = ctx.workspace_root().join("crates/picloud-sdk-gen/src");
        if !sdk_gen_dir.exists() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "picloud-sdk-gen crate not found".to_string(),
            };
        }

        // 2. Read the implementation source
        let impl_path = sdk_gen_dir.join("implementation.rs");
        let impl_content = match std::fs::read_to_string(&impl_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", impl_path.display(), e),
                };
            }
        };

        // 3. Verify SdkGenerator struct exists
        if !impl_content.contains("struct SdkGenerator") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "SdkGenerator struct not found in sdk-gen implementation".to_string(),
            };
        }

        // 4. Verify all three language targets exist
        let languages = ["Rust", "TypeScript", "DotNet"];
        for lang in &languages {
            if !impl_content.contains(lang) {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("SdkLanguage missing target: {}", lang),
                };
            }
        }

        // 5. Verify SdkLanguage enum exists
        if !impl_content.contains("enum SdkLanguage") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "SdkLanguage enum not found".to_string(),
            };
        }

        // 6. Verify generate method exists
        if !impl_content.contains("fn generate") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "SdkGenerator missing generate method".to_string(),
            };
        }

        // 7. Verify generate_all method exists (for all languages)
        if !impl_content.contains("fn generate_all") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "SdkGenerator missing generate_all method".to_string(),
            };
        }

        // 8. Verify SdkGenerationResult type exists
        if !impl_content.contains("struct SdkGenerationResult") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "SdkGenerationResult type not found".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
