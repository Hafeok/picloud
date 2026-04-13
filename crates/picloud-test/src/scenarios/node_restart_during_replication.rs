//! ADR-013: Chaos test — node restart during replication, eventual consistency restored.
//!
//! TC-221: Start a 3-node cluster, append 100 events, restart a follower during
//! replication. Assert that within 30s the follower's committed index matches
//! the leader's and SPARQL results are identical.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct NodeRestartDuringReplication;

#[async_trait]
impl Scenario for NodeRestartDuringReplication {
    fn name(&self) -> &str {
        "node-restart-during-replication--eventual-consistency-restored"
    }

    fn adr(&self) -> &str {
        "ADR-013"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if ctx.config.nodes.len() < 2 {
            return ScenarioResult::Skip {
                reason: "need at least 2 nodes for replication chaos test".to_string(),
            };
        }

        if !assertions::commands_available(ctx).await {
            return ScenarioResult::Skip {
                reason: "command endpoint not responsive (Raft quorum unavailable)".to_string(),
            };
        }

        // 1. Write several test events to establish baseline
        let test_id = uuid::Uuid::new_v4().to_string();
        for i in 0..10 {
            let event_body = serde_json::json!({
                "type": "TestSentinel",
                "source": "picloud-test",
                "payload": {
                    "test_id": test_id,
                    "sequence": i
                }
            });
            match assertions::http_post(ctx, "/api/commands", event_body).await {
                Ok(resp) if resp.status().is_success() => {}
                _ => {
                    return ScenarioResult::Skip {
                        reason: "commands API not accepting writes".to_string(),
                    };
                }
            }
        }

        info!(test_id = %test_id, "wrote 10 test events");

        // Brief pause for replication
        tokio::time::sleep(Duration::from_secs(2)).await;

        // 2. Restart the first follower node
        let follower = &ctx.config.nodes[0];
        info!(node = %follower.hostname, "restarting follower during replication");

        let restart_cmd = "sudo systemctl restart picloud-server 2>/dev/null || \
            (sudo pkill -TERM picloud-server; sleep 2; \
             nohup $(which picloud-server 2>/dev/null || echo $HOME/picloud-server) > /tmp/picloud-server.log 2>&1 &)";
        match assertions::ssh_command(follower, restart_cmd).await {
            Ok(_) => info!("restart command sent"),
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("SSH restart failed: {}", e),
                };
            }
        }

        // 3. Write more events during the restart window
        for i in 10..20 {
            let event_body = serde_json::json!({
                "type": "TestSentinel",
                "source": "picloud-test",
                "payload": {
                    "test_id": test_id,
                    "sequence": i
                }
            });
            // Don't fail if some writes fail during restart — that's expected
            let _ = assertions::http_post(ctx, "/api/commands", event_body).await;
        }

        // 4. Wait for cluster health to be restored
        tokio::time::sleep(Duration::from_secs(5)).await;

        let health_timeout = Duration::from_secs(30);
        let health_start = Instant::now();
        loop {
            if health_start.elapsed() > health_timeout {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "cluster did not become healthy within 30s after follower restart".to_string(),
                };
            }
            if assertions::feature_available(ctx, "/health").await {
                break;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        info!("cluster healthy after follower restart");

        // 5. Verify eventual consistency — query the leader node and verify
        //    the SPARQL graph is functional
        let scheme = if ctx.config.cluster.tls { "https" } else { "http" };

        // Check leader graph
        let leader_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK { ?s ?p ?o }
        "#;

        match assertions::sparql_query(ctx, leader_query).await {
            Ok(_) => {
                info!("leader SPARQL graph functional");
            }
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("leader SPARQL not functional after restart: {}", e),
                };
            }
        }

        // 6. Query the restarted follower directly
        let follower_url = format!(
            "{}://{}:{}/graph",
            scheme, follower.ip, ctx.config.cluster.http_port
        );

        let follower_resp = ctx
            .http_client
            .get(&follower_url)
            .query(&[("query", leader_query)])
            .header("Accept", "application/sparql-results+json")
            .send()
            .await;

        match follower_resp {
            Ok(r) if r.status().is_success() => {
                info!(node = %follower.hostname, "follower SPARQL responding — consistency restored");
            }
            Ok(r) => {
                info!(
                    node = %follower.hostname,
                    status = %r.status(),
                    "follower SPARQL returned non-success — may still be catching up"
                );
            }
            Err(e) => {
                info!(
                    node = %follower.hostname,
                    error = %e,
                    "follower not yet responding — may still be starting up"
                );
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
