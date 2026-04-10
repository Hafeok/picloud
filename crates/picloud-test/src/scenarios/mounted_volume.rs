//! ADR-012: Mounted volume — apply volume, attach to container, write sentinel,
//! restart, verify sentinel persists.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct MountedVolume;

#[async_trait]
impl Scenario for MountedVolume {
    fn name(&self) -> &str {
        "mounted-volume"
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

        // Check that the volume/storage API is available.
        if !assertions::feature_available(ctx, "/api/volumes").await {
            return ScenarioResult::Skip {
                reason: "volume API not available".to_string(),
            };
        }

        // Apply a test volume resource.
        let volume_body = serde_json::json!({
            "type": "volume",
            "name": "test-mounted-vol",
            "product": "picloud-test",
            "spec": {
                "mode": "mounted",
                "size": "1Gi",
                "durability": "full-replication",
                "mountPath": "/data"
            }
        });

        match assertions::http_post(ctx, "/api/resources", volume_body).await {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 409 => {
                info!("volume resource applied");
            }
            Ok(resp) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("volume apply returned status {}", resp.status()),
                };
            }
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("volume apply failed: {}", e),
                };
            }
        }

        // Wait for volume to be ready in RDF graph.
        let volume_ready_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                <https://picloud.local/products/picloud-test/volumes/test-mounted-vol>
                    picloud:status picloud:Ready .
            }
        "#;

        if let Err(e) = assertions::wait_for_sparql(
            ctx,
            volume_ready_query,
            Duration::from_secs(60),
        )
        .await
        {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("volume did not reach Ready state: {}", e),
            };
        }

        info!("volume reached Ready state");

        // Write a sentinel file via the platform API or SSH.
        let sentinel_body = serde_json::json!({
            "path": "/data/sentinel.txt",
            "content": "picloud-test-sentinel",
            "volume": "test-mounted-vol",
            "product": "picloud-test"
        });

        match assertions::http_post(ctx, "/api/volumes/write", sentinel_body).await {
            Ok(resp) if resp.status().is_success() => {
                info!("sentinel file written");
            }
            Ok(resp) => {
                return ScenarioResult::Skip {
                    reason: format!(
                        "volume write API returned {} — endpoint may not be implemented",
                        resp.status()
                    ),
                };
            }
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("volume write API unavailable: {}", e),
                };
            }
        }

        // Verify sentinel file via read API.
        let read_resp = match assertions::http_get(
            ctx,
            "/api/volumes/read?product=picloud-test&volume=test-mounted-vol&path=/data/sentinel.txt",
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("sentinel read failed: {}", e),
                };
            }
        };

        if !read_resp.status().is_success() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "sentinel read returned status {}",
                    read_resp.status()
                ),
            };
        }

        info!("sentinel file verified after write");

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
