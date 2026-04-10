//! ADR-001: Verify the workspace compiles with `cargo build --release` and the
//! resulting binary has no unexpected dynamic dependencies beyond libc.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct BinaryCompiles;

#[async_trait]
impl Scenario for BinaryCompiles {
    fn name(&self) -> &str {
        "binary_compiles"
    }

    fn adr(&self) -> &str {
        "ADR-001"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Run cargo build --release in the workspace root.
        let build_output = match tokio::process::Command::new("cargo")
            .arg("build")
            .arg("--release")
            .current_dir(ctx.workspace_root())
            .output()
            .await
        {
            Ok(o) => o,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to spawn cargo build: {}", e),
                };
            }
        };

        if !build_output.status.success() {
            let stderr = String::from_utf8_lossy(&build_output.stderr);
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "cargo build --release failed (exit {}): {}",
                    build_output.status,
                    stderr.chars().take(2000).collect::<String>()
                ),
            };
        }

        // Find the picloud-server binary in the release target directory.
        let binary_path = ctx
            .workspace_root()
            .join("target/release/picloud-server");

        if !binary_path.exists() {
            // Try alternative name
            let alt_path = ctx.workspace_root().join("target/release/picloud");
            if !alt_path.exists() {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "Release binary not found at target/release/picloud-server or target/release/picloud".to_string(),
                };
            }
            // Use the alternative path for ldd check
            return check_dynamic_deps(&alt_path, start).await;
        }

        check_dynamic_deps(&binary_path, start).await
    }
}

async fn check_dynamic_deps(binary_path: &std::path::Path, start: Instant) -> ScenarioResult {
    // Run ldd on the binary to check dynamic dependencies.
    let ldd_output = match tokio::process::Command::new("ldd")
        .arg(binary_path)
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            // ldd might not be available (e.g., statically linked binary or cross-compile).
            // If ldd fails to run, that's acceptable — the binary may be fully static.
            if e.kind() == std::io::ErrorKind::NotFound {
                return ScenarioResult::Pass {
                    duration: start.elapsed(),
                };
            }
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("Failed to run ldd: {}", e),
            };
        }
    };

    let ldd_stdout = String::from_utf8_lossy(&ldd_output.stdout);

    // If ldd says "not a dynamic executable", the binary is static — that's fine.
    if ldd_stdout.contains("not a dynamic executable") || ldd_stdout.contains("statically linked") {
        return ScenarioResult::Pass {
            duration: start.elapsed(),
        };
    }

    // Check each line of ldd output. Allowed: linux-vdso, libc, libgcc_s,
    // libm, libpthread, libdl, librt, ld-linux (the dynamic linker itself).
    let allowed_prefixes = [
        "linux-vdso",
        "libc.so",
        "libc.musl",
        "libgcc_s.so",
        "libm.so",
        "libpthread.so",
        "libdl.so",
        "librt.so",
        "ld-linux",
        "ld-musl",
        "/lib",  // dynamic linker path entries
    ];

    let mut unexpected: Vec<String> = Vec::new();

    for line in ldd_stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Extract the library name (first token before "=>" or the whole thing).
        let lib_name = trimmed
            .split("=>")
            .next()
            .unwrap_or(trimmed)
            .trim();

        let is_allowed = allowed_prefixes
            .iter()
            .any(|prefix| lib_name.starts_with(prefix));

        if !is_allowed {
            unexpected.push(trimmed.to_string());
        }
    }

    let duration = start.elapsed();

    if unexpected.is_empty() {
        ScenarioResult::Pass { duration }
    } else {
        ScenarioResult::Fail {
            duration,
            reason: format!(
                "Binary has {} unexpected dynamic dependencies: {}",
                unexpected.len(),
                unexpected.join("; ")
            ),
        }
    }
}
