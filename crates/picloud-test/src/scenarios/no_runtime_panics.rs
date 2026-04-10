//! ADR-001: Grep workspace source for `unwrap()`, `expect()`, and `panic!()`
//! calls in non-test production code. Report counts and fail if `panic!()`
//! calls exist in production code paths.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct NoRuntimePanics;

#[async_trait]
impl Scenario for NoRuntimePanics {
    fn name(&self) -> &str {
        "no_runtime_panics"
    }

    fn adr(&self) -> &str {
        "ADR-001"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        let crates_dir = ctx.workspace_root().join("crates");
        let src_main = ctx.workspace_root().join("src");

        // Collect all production .rs files (skip test files, examples, benches,
        // and the picloud-test crate itself).
        let mut prod_files: Vec<std::path::PathBuf> = Vec::new();

        for dir in [&crates_dir, &src_main] {
            if !dir.exists() {
                continue;
            }
            collect_rs_files(dir, &mut prod_files);
        }

        let mut unwrap_count: usize = 0;
        let mut expect_count: usize = 0;
        let mut panic_locations: Vec<String> = Vec::new();

        for file in &prod_files {
            let path_str = file.to_string_lossy();

            // Skip test crate, test modules, examples, benches.
            if path_str.contains("picloud-test")
                || path_str.contains("/tests/")
                || path_str.contains("/examples/")
                || path_str.contains("/benches/")
            {
                continue;
            }

            let content = match std::fs::read_to_string(file) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Track whether we are inside a #[cfg(test)] module or a #[test] fn.
            let mut in_test_block = false;
            let mut brace_depth: i32 = 0;
            let mut test_start_depth: Option<i32> = None;

            for (line_num, line) in content.lines().enumerate() {
                let trimmed = line.trim();

                // Detect #[cfg(test)] or #[test] attribute.
                if trimmed.starts_with("#[cfg(test)]") || trimmed.starts_with("#[test]") {
                    in_test_block = true;
                    test_start_depth = Some(brace_depth);
                }

                // Track brace depth for scoping.
                for ch in line.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => {
                            brace_depth -= 1;
                            if let Some(start) = test_start_depth {
                                if brace_depth <= start {
                                    in_test_block = false;
                                    test_start_depth = None;
                                }
                            }
                        }
                        _ => {}
                    }
                }

                if in_test_block {
                    continue;
                }

                // Skip comment lines.
                if trimmed.starts_with("//") {
                    continue;
                }

                // Count unwrap() and expect() in production code.
                if trimmed.contains(".unwrap()") {
                    unwrap_count += 1;
                }
                if trimmed.contains(".expect(") {
                    expect_count += 1;
                }

                // Check for panic!() macro in production code.
                if trimmed.contains("panic!(") {
                    let relative = file
                        .strip_prefix(ctx.workspace_root())
                        .unwrap_or(file)
                        .to_string_lossy();
                    panic_locations.push(format!("{}:{}", relative, line_num + 1));
                }
            }
        }

        let duration = start.elapsed();

        if !panic_locations.is_empty() {
            return ScenarioResult::Fail {
                duration,
                reason: format!(
                    "Found {} panic!() call(s) in production code: {}. \
                     Also found {} unwrap() and {} expect() calls (informational).",
                    panic_locations.len(),
                    panic_locations.join(", "),
                    unwrap_count,
                    expect_count
                ),
            };
        }

        ScenarioResult::Pass { duration }
    }
}

/// Recursively collect all `.rs` files under a directory.
fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}
