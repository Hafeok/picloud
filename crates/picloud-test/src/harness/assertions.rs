use std::net::IpAddr;
use std::time::Duration;

use hickory_resolver::config::{NameServerConfig, ResolverConfig};
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::proto::xfer::Protocol;
use hickory_resolver::TokioResolver;

use crate::config::NodeConfig;
use crate::harness::runner::TestContext;

/// POST a SPARQL query to the cluster and return the raw response body.
pub async fn sparql_query(
    ctx: &TestContext,
    query: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{}/sparql", ctx.config.base_url());
    let resp = ctx
        .http_client
        .post(&url)
        .header("Content-Type", "application/sparql-query")
        .header("Accept", "application/sparql-results+json")
        .body(query.to_string())
        .send()
        .await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(format!("SPARQL query failed ({}): {}", status, body).into());
    }
    Ok(body)
}

/// GET from a cluster HTTP endpoint.
pub async fn http_get(
    ctx: &TestContext,
    path: &str,
) -> Result<reqwest::Response, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{}{}", ctx.config.base_url(), path);
    let resp = ctx.http_client.get(&url).send().await?;
    Ok(resp)
}

/// POST to a cluster HTTP endpoint with a JSON body.
pub async fn http_post(
    ctx: &TestContext,
    path: &str,
    body: serde_json::Value,
) -> Result<reqwest::Response, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{}{}", ctx.config.base_url(), path);
    let resp = ctx.http_client.post(&url).json(&body).send().await?;
    Ok(resp)
}

/// Resolve a hostname using the cluster's DNS (first node IP, port 53).
pub async fn dns_lookup(
    ctx: &TestContext,
    hostname: &str,
) -> Result<Vec<IpAddr>, Box<dyn std::error::Error + Send + Sync>> {
    let node_ip: IpAddr = ctx
        .config
        .first_node_ip()
        .ok_or("no nodes configured")?
        .parse()?;

    let ns = NameServerConfig::new(
        std::net::SocketAddr::new(node_ip, 53),
        Protocol::Udp,
    );
    let mut resolver_config = ResolverConfig::new();
    resolver_config.add_name_server(ns);

    let resolver = TokioResolver::builder_with_config(
        resolver_config,
        TokioConnectionProvider::default(),
    )
    .build();
    let response = resolver.lookup_ip(hostname).await?;
    let addrs: Vec<IpAddr> = response.iter().collect();
    Ok(addrs)
}

/// Assert that a SPARQL COUNT query returns the expected integer value.
///
/// The query must return a single binding named `count`, e.g.:
/// `SELECT (COUNT(?s) AS ?count) WHERE { ?s a :Container }`
pub async fn assert_sparql_count(
    ctx: &TestContext,
    query: &str,
    expected: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let body = sparql_query(ctx, query).await?;
    let json: serde_json::Value = serde_json::from_str(&body)?;

    let count_value = json
        .pointer("/results/bindings/0/count/value")
        .and_then(|v| v.as_str())
        .ok_or("SPARQL response missing /results/bindings/0/count/value")?;

    let actual: u64 = count_value.parse()?;
    if actual != expected {
        return Err(format!(
            "SPARQL count mismatch: expected {}, got {}",
            expected, actual
        )
        .into());
    }
    Ok(())
}

/// Run an SSH command on a node and return stdout.
pub async fn ssh_command(node: &NodeConfig, command: &str) -> Result<String, String> {
    let target = format!("{}@{}", node.ssh_user, node.ip);
    let output = tokio::process::Command::new("ssh")
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg(&target)
        .arg(command)
        .output()
        .await
        .map_err(|e| format!("ssh to {} failed: {}", node.hostname, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(format!(
            "ssh command on {} exited {}: stdout={}, stderr={}",
            node.hostname,
            output.status,
            stdout.trim(),
            stderr.trim()
        ));
    }

    Ok(stdout)
}

/// Check if a feature endpoint is available (not 404, 501, or connection error).
pub async fn feature_available(
    ctx: &TestContext,
    path: &str,
) -> bool {
    match http_get(ctx, path).await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            status != 404 && status != 501
        }
        Err(_) => false,
    }
}

/// Poll a SPARQL ASK query until it returns true, with timeout.
pub async fn wait_for_sparql(
    ctx: &TestContext,
    ask_query: &str,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > timeout {
            return Err(format!(
                "SPARQL ASK did not return true within {:?}",
                timeout
            )
            .into());
        }

        match sparql_query(ctx, ask_query).await {
            Ok(body) => {
                let json: serde_json::Value = serde_json::from_str(&body)?;
                if json.get("boolean").and_then(|v| v.as_bool()) == Some(true) {
                    return Ok(());
                }
            }
            Err(_) => {} // keep polling
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Assert that an HTTP GET to the given path returns the expected status code.
pub async fn assert_http_status(
    ctx: &TestContext,
    path: &str,
    expected_status: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let resp = http_get(ctx, path).await?;
    let actual = resp.status().as_u16();
    if actual != expected_status {
        return Err(format!(
            "HTTP status mismatch for {}: expected {}, got {}",
            path, expected_status, actual
        )
        .into());
    }
    Ok(())
}
