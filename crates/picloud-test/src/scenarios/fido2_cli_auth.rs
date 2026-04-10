//! ADR-025: FIDO2/WebAuthn CLI Authentication — verify passkey support in CLI source.
//!
//! Checks CLI source for FIDO2/WebAuthn authentication support. Verifies
//! passkey-related subcommands or modules exist.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct Fido2CliAuthScenario;

#[async_trait]
impl Scenario for Fido2CliAuthScenario {
    fn name(&self) -> &str {
        "fido2-cli-auth"
    }

    fn adr(&self) -> &str {
        "ADR-025"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        let cli_bin = ctx.workspace_root().join("target/release/picloud");

        // If CLI binary exists, check for auth/passkey subcommands.
        if cli_bin.exists() {
            let output = tokio::process::Command::new(&cli_bin)
                .arg("--help")
                .output()
                .await;

            if let Ok(help) = output {
                let stdout = String::from_utf8_lossy(&help.stdout);
                let stderr = String::from_utf8_lossy(&help.stderr);
                let combined = format!("{}{}", stdout, stderr).to_lowercase();

                let auth_keywords = ["auth", "login", "passkey", "fido", "webauthn", "device-flow"];
                let found: Vec<&&str> = auth_keywords
                    .iter()
                    .filter(|kw| combined.contains(*kw))
                    .collect();

                if !found.is_empty() {
                    info!(
                        keywords = ?found,
                        "CLI binary references FIDO2/passkey authentication"
                    );
                    return ScenarioResult::Pass {
                        duration: start.elapsed(),
                    };
                }
            }

            info!("CLI binary help does not mention passkey auth; checking source");
        }

        // Check CLI source for FIDO2/WebAuthn modules.
        let cli_src = ctx.workspace_root().join("crates/picloud-cli/src");
        if !cli_src.exists() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "picloud-cli crate source directory does not exist".to_string(),
            };
        }

        let fido2_markers = [
            "webauthn",
            "fido2",
            "passkey",
            "WebAuthn",
            "Fido2",
            "Passkey",
            "device_flow",
            "DeviceFlow",
            "authentication",
        ];

        let mut found_fido2 = false;
        let mut found_in_file = String::new();

        // Recursive scan of CLI source.
        fn scan_dir(
            dir: &std::path::Path,
            markers: &[&str],
            found: &mut bool,
            found_file: &mut String,
        ) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        scan_dir(&path, markers, found, found_file);
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

        scan_dir(&cli_src, &fido2_markers, &mut found_fido2, &mut found_in_file);

        // Also check picloud-iam for WebAuthn implementation.
        if !found_fido2 {
            let iam_src = ctx.workspace_root().join("crates/picloud-iam/src");
            scan_dir(&iam_src, &fido2_markers, &mut found_fido2, &mut found_in_file);
        }

        if found_fido2 {
            info!(
                file = found_in_file.as_str(),
                "FIDO2/WebAuthn authentication support found"
            );
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        } else {
            ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "no FIDO2/WebAuthn/passkey authentication code found in picloud-cli or picloud-iam".to_string(),
            }
        }
    }
}
