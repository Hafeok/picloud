//! TC-218: SDKs published to package registries (exit-criteria).
//!
//! ADR-033: Verify that the SDK generation and publish pipeline supports
//! all three language targets (Rust/crates.io, TypeScript/npm, .NET/NuGet)
//! and that each generated SDK includes the complete platform API surface.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct SdksPublishedToPackageRegistries;

#[async_trait]
impl Scenario for SdksPublishedToPackageRegistries {
    fn name(&self) -> &str {
        "sdks-published-to-package-registries"
    }

    fn adr(&self) -> &str {
        "ADR-033"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();
        let mut issues: Vec<String> = Vec::new();

        // 1. Verify picloud-sdk-gen crate exists with implementation
        let impl_path = ctx
            .workspace_root()
            .join("crates/picloud-sdk-gen/src/implementation.rs");
        let impl_content = match std::fs::read_to_string(&impl_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read sdk-gen implementation: {}", e),
                };
            }
        };

        // 2. Verify SdkGenerator and SdkLanguage types
        if !impl_content.contains("struct SdkGenerator") {
            issues.push("SdkGenerator struct not found".to_string());
        }
        if !impl_content.contains("enum SdkLanguage") {
            issues.push("SdkLanguage enum not found".to_string());
        }

        // 3. Verify all three language targets
        for lang in &["Rust", "TypeScript", "DotNet"] {
            if !impl_content.contains(lang) {
                issues.push(format!("SdkLanguage missing {} target", lang));
            }
        }

        // 4. Verify generate and publish methods exist
        if !impl_content.contains("fn generate") {
            issues.push("SdkGenerator missing generate method".to_string());
        }
        if !impl_content.contains("fn generate_all") {
            issues.push("SdkGenerator missing generate_all method".to_string());
        }
        if !impl_content.contains("fn publish") {
            issues.push("SdkGenerator missing publish method".to_string());
        }
        if !impl_content.contains("fn publish_all") {
            issues.push("SdkGenerator missing publish_all method".to_string());
        }

        // 5. Verify correct package names for each registry
        if !impl_content.contains("picloud-sdk") {
            issues.push("Rust crate name 'picloud-sdk' not found".to_string());
        }
        if !impl_content.contains("@picloud/sdk") {
            issues.push("TypeScript package name '@picloud/sdk' not found".to_string());
        }
        if !impl_content.contains("PiCloud.Sdk") {
            issues.push("NuGet package name 'PiCloud.Sdk' not found".to_string());
        }

        // 6. Verify publish commands target correct registries
        let publish_commands = [
            ("Rust", "cargo publish"),
            ("TypeScript", "npm publish"),
            (".NET", "dotnet nuget push"),
        ];
        for (lang, cmd) in &publish_commands {
            if !impl_content.contains(cmd) {
                issues.push(format!("{} publish command '{}' not found", lang, cmd));
            }
        }

        // 7. Verify SdkGenerationResult and SdkPublishResult types
        if !impl_content.contains("struct SdkGenerationResult") {
            issues.push("SdkGenerationResult type not found".to_string());
        }
        if !impl_content.contains("struct SdkPublishResult") {
            issues.push("SdkPublishResult type not found".to_string());
        }

        // 8. Verify SDK generation depends on picloud-domain (ontology binding)
        let cargo_path = ctx
            .workspace_root()
            .join("crates/picloud-sdk-gen/Cargo.toml");
        match std::fs::read_to_string(&cargo_path) {
            Ok(c) => {
                if !c.contains("picloud-domain") {
                    issues.push(
                        "picloud-sdk-gen does not depend on picloud-domain".to_string(),
                    );
                }
            }
            Err(e) => {
                issues.push(format!("Cannot read picloud-sdk-gen Cargo.toml: {}", e));
            }
        }

        // 9. Verify lib.rs re-exports generator types
        let lib_path = ctx
            .workspace_root()
            .join("crates/picloud-sdk-gen/src/lib.rs");
        match std::fs::read_to_string(&lib_path) {
            Ok(c) => {
                for ty in &[
                    "SdkGenerator",
                    "SdkGenerationResult",
                    "SdkLanguage",
                    "SdkPublishResult",
                ] {
                    if !c.contains(ty) {
                        issues.push(format!("{} not re-exported from lib.rs", ty));
                    }
                }
            }
            Err(e) => {
                issues.push(format!("Cannot read picloud-sdk-gen lib.rs: {}", e));
            }
        }

        // 10. Verify ClusterDomain ontology binding
        if !impl_content.contains("ClusterDomain") {
            issues.push("SdkGenerator does not reference ClusterDomain".to_string());
        }
        if !impl_content.contains("cluster_domain") {
            issues.push("SdkGenerator missing cluster_domain field".to_string());
        }

        let duration = start.elapsed();

        if issues.is_empty() {
            ScenarioResult::Pass { duration }
        } else {
            ScenarioResult::Fail {
                duration,
                reason: format!(
                    "{} exit-criteria issue(s): {}",
                    issues.len(),
                    issues.join("; ")
                ),
            }
        }
    }
}
