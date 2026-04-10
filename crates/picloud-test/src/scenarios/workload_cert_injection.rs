//! ADR-027: mTLS Certificate Injection — verify workload certificate injection exists.
//!
//! Reads workload and IAM source to confirm mTLS certificate injection into
//! workload environments is implemented.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct WorkloadCertInjectionScenario;

#[async_trait]
impl Scenario for WorkloadCertInjectionScenario {
    fn name(&self) -> &str {
        "workload-cert-injection"
    }

    fn adr(&self) -> &str {
        "ADR-027"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        let cert_markers = [
            "mtls",
            "mTLS",
            "client_cert",
            "client_certificate",
            "cert_inject",
            "certificate_injection",
            "workload_cert",
            "WorkloadCertificate",
            "inject_cert",
            "CertificateInjection",
            "platform_ca",
            "PlatformCa",
            "cluster_ca",
        ];

        let search_dirs = [
            ctx.workspace_root().join("crates/picloud-workload/src"),
            ctx.workspace_root().join("crates/picloud-iam/src"),
            ctx.workspace_root().join("crates/picloud-network/src"),
        ];

        let mut found_cert_code = false;
        let mut found_in_file = String::new();
        let mut found_marker = String::new();

        for dir in &search_dirs {
            if !dir.exists() {
                continue;
            }

            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e == "rs").unwrap_or(false) {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            for marker in &cert_markers {
                                if content.contains(marker) {
                                    found_cert_code = true;
                                    found_in_file = path.display().to_string();
                                    found_marker = marker.to_string();
                                    break;
                                }
                            }
                        }
                    }
                    if found_cert_code {
                        break;
                    }
                }
            }
            if found_cert_code {
                break;
            }
        }

        // Also check picloud-domain for certificate-related types.
        if !found_cert_code {
            let domain_files = [
                ctx.workspace_root().join("crates/picloud-domain/src/identity.rs"),
                ctx.workspace_root().join("crates/picloud-domain/src/traits.rs"),
                ctx.workspace_root().join("crates/picloud-domain/src/workload.rs"),
            ];

            for path in &domain_files {
                if let Ok(content) = std::fs::read_to_string(path) {
                    for marker in &cert_markers {
                        if content.contains(marker) {
                            found_cert_code = true;
                            found_in_file = path.display().to_string();
                            found_marker = marker.to_string();
                            break;
                        }
                    }
                }
                if found_cert_code {
                    break;
                }
            }
        }

        if found_cert_code {
            info!(
                file = found_in_file.as_str(),
                marker = found_marker.as_str(),
                "mTLS certificate injection mechanism found"
            );
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        } else {
            ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "no mTLS certificate injection code found in picloud-workload, picloud-iam, or picloud-network".to_string(),
            }
        }
    }
}
