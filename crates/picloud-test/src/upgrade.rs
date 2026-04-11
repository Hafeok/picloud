use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;
use tracing::{error, info, warn};

use crate::config::{ClusterConfig, NodeConfig};
use crate::harness::runner::TestContext;

/// Configuration for a rolling upgrade operation.
pub struct UpgradeConfig {
    pub binary_path: PathBuf,
    pub cluster_config: ClusterConfig,
}

/// Report summarizing the full rolling upgrade.
pub struct UpgradeReport {
    pub nodes: Vec<NodeUpgradeResult>,
    pub total_duration: Duration,
    pub success: bool,
}

/// Per-node result of the upgrade process.
pub struct NodeUpgradeResult {
    pub hostname: String,
    pub role: String,
    pub scp_duration: Duration,
    pub restart_duration: Duration,
    pub rejoin_duration: Duration,
    pub success: bool,
    pub error: Option<String>,
}

impl UpgradeReport {
    /// Print a human-readable summary of the upgrade.
    pub fn print(&self) {
        println!();
        println!("=== Rolling Upgrade Report ===");
        println!(
            "  Total duration: {:.2}s",
            self.total_duration.as_secs_f64()
        );
        println!(
            "  Result: {}",
            if self.success { "SUCCESS" } else { "FAILED" }
        );
        println!();
        println!(
            "  {:<15} {:<10} {:<10} {:<10} {:<10} {}",
            "NODE", "ROLE", "SCP", "RESTART", "REJOIN", "STATUS"
        );
        println!("  {}", "-".repeat(70));
        for node in &self.nodes {
            println!(
                "  {:<15} {:<10} {:<10} {:<10} {:<10} {}",
                node.hostname,
                node.role,
                format!("{:.1}s", node.scp_duration.as_secs_f64()),
                format!("{:.1}s", node.restart_duration.as_secs_f64()),
                format!("{:.1}s", node.rejoin_duration.as_secs_f64()),
                if node.success {
                    "OK".to_string()
                } else {
                    format!("FAIL: {}", node.error.as_deref().unwrap_or("unknown"))
                },
            );
        }
        println!("==============================");
    }
}

/// Determine which node is the current Raft leader by querying SPARQL.
///
/// Queries for a node with `picloud:hasRole "leader"` in the RDF graph.
/// Returns the hostname of the leader node.
async fn find_leader(ctx: &TestContext) -> Result<String, Box<dyn std::error::Error>> {
    let query = r#"
        PREFIX picloud: <https://picloud.local/ontology#>
        SELECT ?hostname WHERE {
            ?node a picloud:Node ;
                  picloud:hasRole "leader" ;
                  picloud:hostname ?hostname .
        }
        LIMIT 1
    "#;

    let url = format!("{}/graph", ctx.config.base_url());
    let resp = ctx
        .http_client
        .get(&url)
        .query(&[("query", query)])
        .header("Accept", "application/sparql-results+json")
        .send()
        .await?;

    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(format!("SPARQL leader query failed ({}): {}", status, body).into());
    }

    // Normalize: server may return {"results":[...]} instead of {"results":{"bindings":[...]}}
    let mut json: Value = serde_json::from_str(&body)?;
    if let Some(results) = json.get("results").cloned() {
        if results.is_array() {
            json["results"] = serde_json::json!({"bindings": results});
        }
    }

    let hostname = json
        .pointer("/results/bindings/0/hostname/value")
        .and_then(|v| v.as_str())
        .ok_or("Could not determine Raft leader from SPARQL response")?;

    Ok(hostname.to_string())
}

/// Copy a binary to a remote node via scp.
async fn scp_binary(node: &NodeConfig, binary_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let dest = format!(
        "{}@{}:/tmp/picloud-server.new",
        node.ssh_user, node.ip
    );

    info!(
        node = node.hostname.as_str(),
        ip = node.ip.as_str(),
        "copying binary via scp"
    );

    let output = tokio::process::Command::new("scp")
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg(binary_path)
        .arg(&dest)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("scp to {} failed: {}", node.hostname, stderr).into());
    }

    Ok(())
}

/// Run a command on a remote node via ssh.
async fn ssh_command(
    node: &NodeConfig,
    command: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let target = format!("{}@{}", node.ssh_user, node.ip);

    let output = tokio::process::Command::new("ssh")
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg(&target)
        .arg(command)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "ssh command on {} failed: {}",
            node.hostname, stderr
        )
        .into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout)
}

/// Restart picloud-server on a node: stop, replace binary, start.
///
/// Supports both systemd-managed and nohup-managed processes. Tries systemctl
/// first; if the service unit doesn't exist, falls back to pkill + nohup.
async fn restart_node(node: &NodeConfig) -> Result<(), Box<dyn std::error::Error>> {
    // Detect whether systemd manages picloud-server on this node.
    let has_systemd = match ssh_command(
        node,
        "systemctl list-unit-files picloud-server.service 2>/dev/null | grep -q picloud-server && echo yes || echo no",
    )
    .await
    {
        Ok(out) => out.trim() == "yes",
        Err(_) => false,
    };

    if has_systemd {
        info!(node = node.hostname.as_str(), "stopping picloud-server (systemd)");
        ssh_command(node, "sudo systemctl stop picloud-server").await?;

        info!(node = node.hostname.as_str(), "replacing binary");
        ssh_command(
            node,
            "sudo mv /tmp/picloud-server.new /usr/local/bin/picloud-server && sudo chmod +x /usr/local/bin/picloud-server",
        )
        .await?;

        info!(node = node.hostname.as_str(), "starting picloud-server (systemd)");
        ssh_command(node, "sudo systemctl start picloud-server").await?;
    } else {
        info!(node = node.hostname.as_str(), "stopping picloud-server (pkill)");
        // pkill may exit non-zero if process isn't running; that's fine.
        let _ = ssh_command(node, "sudo pkill -TERM picloud-server || true").await;
        // Give it a moment to shut down gracefully.
        tokio::time::sleep(Duration::from_secs(2)).await;
        let _ = ssh_command(node, "sudo pkill -9 picloud-server || true").await;

        info!(node = node.hostname.as_str(), "replacing binary");
        // Try /usr/local/bin first; fall back to ~/picloud-server
        ssh_command(
            node,
            "if [ -f /usr/local/bin/picloud-server ]; then \
                sudo mv /tmp/picloud-server.new /usr/local/bin/picloud-server && sudo chmod +x /usr/local/bin/picloud-server; \
             else \
                mv /tmp/picloud-server.new ~/picloud-server && chmod +x ~/picloud-server; \
             fi",
        )
        .await?;

        info!(node = node.hostname.as_str(), "starting picloud-server (nohup)");
        // Determine the binary location
        let binary_path = match ssh_command(node, "which picloud-server 2>/dev/null || echo $HOME/picloud-server").await {
            Ok(p) => p.trim().to_string(),
            Err(_) => "~/picloud-server".to_string(),
        };
        ssh_command(
            node,
            &format!("nohup {} > /tmp/picloud-server.log 2>&1 &", binary_path),
        )
        .await?;
    }

    Ok(())
}

/// Wait for a node to rejoin the Raft cluster by polling SPARQL.
///
/// Polls every second until the node appears with a Raft role (leader or follower),
/// or the timeout expires.
async fn wait_for_raft_rejoin(
    ctx: &TestContext,
    node_hostname: &str,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let query = format!(
        r#"
        PREFIX picloud: <https://picloud.local/ontology#>
        SELECT ?role WHERE {{
            ?node a picloud:Node ;
                  picloud:hostname "{}" ;
                  picloud:hasRole ?role .
        }}
        LIMIT 1
        "#,
        node_hostname
    );

    let url = format!("{}/graph", ctx.config.base_url());

    loop {
        if start.elapsed() > timeout {
            return Err(format!(
                "Timeout waiting for {} to rejoin Raft after {:.0}s",
                node_hostname,
                timeout.as_secs_f64()
            )
            .into());
        }

        match ctx
            .http_client
            .get(&url)
            .query(&[("query", &query)])
            .header("Accept", "application/sparql-results+json")
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(body) = resp.text().await {
                    if let Ok(mut json) = serde_json::from_str::<Value>(&body) {
                        // Normalize server format
                        if let Some(results) = json.get("results").cloned() {
                            if results.is_array() {
                                json["results"] = serde_json::json!({"bindings": results});
                            }
                        }
                        if json
                            .pointer("/results/bindings/0/role/value")
                            .and_then(|v| v.as_str())
                            .is_some()
                        {
                            info!(
                                node = node_hostname,
                                elapsed_ms = start.elapsed().as_millis() as u64,
                                "node rejoined Raft"
                            );
                            return Ok(());
                        }
                    }
                }
            }
            Ok(_) => {
                // Non-success status — cluster may still be recovering
            }
            Err(e) => {
                warn!(
                    node = node_hostname,
                    error = %e,
                    "SPARQL poll failed, retrying"
                );
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Verify node health via HTTP health check endpoint.
async fn verify_node_health(
    node: &NodeConfig,
    http_port: u16,
    tls: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let scheme = if tls { "https" } else { "http" };
    let url = format!("{}://{}:{}/health", scheme, node.ip, http_port);

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(10))
        .build()?;

    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        return Err(format!(
            "Health check failed for {} ({}): {}",
            node.hostname,
            resp.status(),
            resp.text().await.unwrap_or_default()
        )
        .into());
    }

    info!(node = node.hostname.as_str(), "health check passed");
    Ok(())
}

/// Execute a rolling upgrade across all cluster nodes.
///
/// Strategy (ADR-055):
/// 1. Sort nodes: followers first, leader last
/// 2. For each node sequentially:
///    a. Copy binary via SCP
///    b. Stop picloud-server
///    c. Replace binary
///    d. Start picloud-server
///    e. Wait for Raft rejoin
///    f. Verify health
/// 3. Halt immediately on any node failure
pub async fn rolling_upgrade(config: UpgradeConfig) -> Result<UpgradeReport, Box<dyn std::error::Error>> {
    let total_start = Instant::now();

    // Validate binary exists
    if !config.binary_path.exists() {
        return Err(format!(
            "Binary not found: {}",
            config.binary_path.display()
        )
        .into());
    }

    let ctx = TestContext::new(config.cluster_config.clone(), std::path::PathBuf::from("."));
    let http_port = config.cluster_config.cluster.http_port;
    let tls = config.cluster_config.cluster.tls;

    // Step 1: Determine the leader so we can upgrade it last
    info!("determining Raft leader");
    let leader_hostname = match find_leader(&ctx).await {
        Ok(h) => {
            info!(leader = h.as_str(), "found Raft leader");
            h
        }
        Err(e) => {
            warn!(error = %e, "could not determine leader, will upgrade in config order");
            String::new()
        }
    };

    // Sort nodes: followers first, leader last
    let mut nodes: Vec<NodeConfig> = config.cluster_config.nodes.clone();
    nodes.sort_by(|a, b| {
        let a_is_leader = a.hostname == leader_hostname;
        let b_is_leader = b.hostname == leader_hostname;
        a_is_leader.cmp(&b_is_leader)
    });

    info!(
        order = ?nodes.iter().map(|n| n.hostname.as_str()).collect::<Vec<_>>(),
        "upgrade order determined"
    );

    let mut results = Vec::new();

    for node in &nodes {
        let is_leader = node.hostname == leader_hostname;
        let role = if is_leader { "leader" } else { "follower" };

        info!(
            node = node.hostname.as_str(),
            role = role,
            "starting upgrade"
        );

        let mut node_result = NodeUpgradeResult {
            hostname: node.hostname.clone(),
            role: role.to_string(),
            scp_duration: Duration::ZERO,
            restart_duration: Duration::ZERO,
            rejoin_duration: Duration::ZERO,
            success: false,
            error: None,
        };

        // Step 2a: Copy binary via SCP
        let scp_start = Instant::now();
        if let Err(e) = scp_binary(node, &config.binary_path).await {
            error!(node = node.hostname.as_str(), error = %e, "SCP failed");
            node_result.scp_duration = scp_start.elapsed();
            node_result.error = Some(format!("SCP failed: {}", e));
            results.push(node_result);
            // Halt on failure
            return Ok(UpgradeReport {
                nodes: results,
                total_duration: total_start.elapsed(),
                success: false,
            });
        }
        node_result.scp_duration = scp_start.elapsed();

        // Step 2b-d: Stop, replace, start
        let restart_start = Instant::now();
        if let Err(e) = restart_node(node).await {
            error!(node = node.hostname.as_str(), error = %e, "restart failed");
            node_result.restart_duration = restart_start.elapsed();
            node_result.error = Some(format!("Restart failed: {}", e));
            results.push(node_result);
            return Ok(UpgradeReport {
                nodes: results,
                total_duration: total_start.elapsed(),
                success: false,
            });
        }
        node_result.restart_duration = restart_start.elapsed();

        // Step 2e: Wait for Raft rejoin
        let rejoin_start = Instant::now();
        if let Err(e) =
            wait_for_raft_rejoin(&ctx, &node.hostname, Duration::from_secs(60)).await
        {
            error!(node = node.hostname.as_str(), error = %e, "Raft rejoin timeout");
            node_result.rejoin_duration = rejoin_start.elapsed();
            node_result.error = Some(format!("Raft rejoin failed: {}", e));
            results.push(node_result);
            return Ok(UpgradeReport {
                nodes: results,
                total_duration: total_start.elapsed(),
                success: false,
            });
        }
        node_result.rejoin_duration = rejoin_start.elapsed();

        // Step 2f: Verify health
        if let Err(e) = verify_node_health(node, http_port, tls).await {
            error!(node = node.hostname.as_str(), error = %e, "health check failed");
            node_result.error = Some(format!("Health check failed: {}", e));
            results.push(node_result);
            return Ok(UpgradeReport {
                nodes: results,
                total_duration: total_start.elapsed(),
                success: false,
            });
        }

        node_result.success = true;
        info!(
            node = node.hostname.as_str(),
            role = role,
            "upgrade complete"
        );
        results.push(node_result);
    }

    Ok(UpgradeReport {
        nodes: results,
        total_duration: total_start.elapsed(),
        success: true,
    })
}
