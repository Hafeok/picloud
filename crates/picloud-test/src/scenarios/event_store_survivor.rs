//! ADR-032: Event store survivor — POST event, restart server, verify event
//! survives.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct EventStoreSurvivor;

#[async_trait]
impl Scenario for EventStoreSurvivor {
    fn name(&self) -> &str {
        "event-store-survivor"
    }

    fn adr(&self) -> &str {
        "ADR-032"
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

        if !assertions::feature_available(ctx, "/api/event-store").await
            && !assertions::feature_available(ctx, "/api/events").await
        {
            return ScenarioResult::Skip {
                reason: "event store/events API not available".to_string(),
            };
        }

        let sentinel_id = format!("survivor-{}", uuid::Uuid::new_v4());

        // Append a sentinel event.
        let event_body = serde_json::json!({
            "type": "SurvivorTestEvent",
            "source": "picloud-test",
            "payload": {
                "sentinelId": sentinel_id
            }
        });

        // Try event store API first, fall back to events API.
        let append_result = assertions::http_post(
            ctx,
            "/api/event-store/picloud-test/append",
            event_body.clone(),
        )
        .await;

        let appended = match append_result {
            Ok(resp) if resp.status().is_success() => true,
            _ => {
                // Fall back to events API.
                match assertions::http_post(ctx, "/api/events", event_body).await {
                    Ok(resp) if resp.status().is_success() => true,
                    Ok(resp) => {
                        return ScenarioResult::Skip {
                            reason: format!(
                                "event append returned status {}",
                                resp.status()
                            ),
                        };
                    }
                    Err(e) => {
                        return ScenarioResult::Skip {
                            reason: format!("event append failed: {}", e),
                        };
                    }
                }
            }
        };

        if !appended {
            return ScenarioResult::Skip {
                reason: "could not append event".to_string(),
            };
        }

        info!(sentinel = %sentinel_id, "sentinel event appended");

        // Wait for projection.
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Restart picloud-server on the first node.
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

        // Wait for recovery.
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

        // Verify the sentinel event survived the restart.
        let sentinel_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                ?event picloud:sentinelId "{}" .
            }}
            "#,
            sentinel_id
        );

        match assertions::wait_for_sparql(
            ctx,
            &sentinel_query,
            Duration::from_secs(30),
        )
        .await
        {
            Ok(()) => {
                info!("sentinel event survived restart");
                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "sentinel event {} not found after restart: {}",
                    sentinel_id, e
                ),
            },
        }
    }
}
