//! ADR-012: Volume mount restart — restart picloud-server, verify volumes survive.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct VolumeMountRestart;

#[async_trait]
impl Scenario for VolumeMountRestart {
    fn name(&self) -> &str {
        "volume-mount-restart"
    }

    fn adr(&self) -> &str {
        "ADR-012"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if ctx.config.nodes.is_empty() {
            return ScenarioResult::Skip {
                reason: "no nodes configured for SSH".to_string(),
            };
        }

        // Ensure there is a product and volume to test with.
        if let Err(e) = assertions::apply_product_and_wait(ctx, "volume-test", "1.0.0").await {
            info!("product apply note: {}", e);
        }
        let volume_resource = serde_json::json!({
            "type": "volume",
            "name": "vol-restart-test",
            "product": "volume-test",
            "size": "1Gi"
        });
        let _ = assertions::apply_resource(ctx, volume_resource).await;

        // Give projection time to catch up.
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Check for existing volumes in the RDF graph before restart.
        let volume_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT (COUNT(?vol) AS ?count) WHERE {
                ?vol a picloud:Volume ;
                     picloud:status picloud:Ready .
            }
        "#;

        let pre_restart_body = match assertions::sparql_query(ctx, volume_query).await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("cannot query volumes before restart: {}", e),
                };
            }
        };

        let pre_json: serde_json::Value = match serde_json::from_str(&pre_restart_body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to parse pre-restart volume count: {}", e),
                };
            }
        };

        let pre_count: u64 = pre_json
            .pointer("/results/bindings/0/count/value")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        if pre_count == 0 {
            return ScenarioResult::Skip {
                reason: "no Ready volumes found — nothing to verify across restart".to_string(),
            };
        }

        info!(count = pre_count, "volumes found before restart");

        // Restart picloud-server on the first node via SSH.
        let node = &ctx.config.nodes[0];
        info!(node = %node.hostname, "restarting picloud-server");

        // Try systemd restart first; fall back to pkill + nohup for non-systemd setups.
        let restart_cmd = "sudo systemctl restart picloud-server 2>/dev/null || \
            (sudo pkill -TERM picloud-server; sleep 2; \
             nohup $(which picloud-server 2>/dev/null || echo $HOME/picloud-server) > /tmp/picloud-server.log 2>&1 &)";
        match assertions::ssh_command(node, restart_cmd).await {
            Ok(_) => info!("restart command sent"),
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("SSH restart failed: {}", e),
                };
            }
        }

        // Wait for the cluster to become healthy again.
        tokio::time::sleep(Duration::from_secs(5)).await;

        let health_timeout = Duration::from_secs(60);
        let health_start = Instant::now();
        loop {
            if health_start.elapsed() > health_timeout {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "cluster did not become healthy within 60s after restart".to_string(),
                };
            }
            if assertions::feature_available(ctx, "/health").await {
                break;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        info!("cluster healthy after restart");

        // Verify volume count matches post-restart.
        let post_restart_body = match assertions::sparql_query(ctx, volume_query).await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to query volumes after restart: {}", e),
                };
            }
        };

        let post_json: serde_json::Value = match serde_json::from_str(&post_restart_body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to parse post-restart volume count: {}", e),
                };
            }
        };

        let post_count: u64 = post_json
            .pointer("/results/bindings/0/count/value")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        if post_count != pre_count {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "volume count changed after restart: {} before, {} after",
                    pre_count, post_count
                ),
            };
        }

        info!(count = post_count, "volume count matches after restart");

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
