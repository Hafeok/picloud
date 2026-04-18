use std::net::IpAddr;
use std::time::Duration;

use hickory_resolver::config::{NameServerConfig, ResolverConfig};
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::proto::xfer::Protocol;
use hickory_resolver::TokioResolver;

use crate::config::NodeConfig;
use crate::harness::runner::TestContext;

/// Execute a SPARQL query against the cluster graph and return the response body.
///
/// Uses GET /graph?query=<sparql> which is the server's SPARQL endpoint.
/// Normalizes the response to standard SPARQL JSON results format so that
/// scenarios can use `/results/bindings/0/...` JSON pointers uniformly.
///
/// Server returns: `{"type":"SparqlResult","results":[{...}]}`
/// Normalized to:  `{"results":{"bindings":[{...}]}}`
pub async fn sparql_query(
    ctx: &TestContext,
    query: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
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
        return Err(format!("SPARQL query failed ({}): {}", status, body).into());
    }

    // Normalize: if server returns {"results":[...]} (flat array),
    // wrap it as {"results":{"bindings":[...]}} for standard SPARQL compat.
    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&body) {
        if let Some(results) = json.get("results").cloned() {
            if results.is_array() {
                // Server format: {"type":"SparqlResult","results":[...]}
                // Standard format: {"results":{"bindings":[...]}}
                json["results"] = serde_json::json!({"bindings": results});
                return Ok(serde_json::to_string(&json)?);
            }
        }
        // Also handle ASK queries: {"type":"SparqlResult","boolean":true/false}
        // Already in standard format, pass through.
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
///
/// Uses a 10-second timeout per request to avoid hanging on Raft consensus
/// issues (e.g., /api/commands blocks waiting for quorum).
pub async fn http_post(
    ctx: &TestContext,
    path: &str,
    body: serde_json::Value,
) -> Result<reqwest::Response, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{}{}", ctx.config.base_url(), path);
    let resp = ctx
        .http_client
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;
    Ok(resp)
}

/// Resolve a hostname using the cluster's DNS (first node IP, port 53).
///
/// Times out after 10 seconds to avoid blocking the test suite when DNS
/// is unreachable (the default hickory timeout retries for minutes).
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

    let response = tokio::time::timeout(
        Duration::from_secs(10),
        resolver.lookup_ip(hostname),
    )
    .await
    .map_err(|_| format!("DNS lookup for {} timed out after 10s", hostname))??;

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

/// Check if the command endpoint is available and responsive.
/// Returns false if POST to /api/commands times out (Raft quorum issue).
pub async fn commands_available(ctx: &TestContext) -> bool {
    // Send a harmless ping-style command to test if Raft can process writes.
    let ping = serde_json::json!({
        "type": "Ping",
        "payload": {}
    });
    match http_post(ctx, "/api/commands", ping).await {
        Ok(resp) => resp.status().is_success() || resp.status().as_u16() == 202,
        Err(_) => false,
    }
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
                // Server may return {"boolean":true} or {"type":"SparqlResult","boolean":true}
                if json.get("boolean").and_then(|v| v.as_bool()) == Some(true) {
                    return Ok(());
                }
                // Fallback: check results array for any bindings (ASK returns boolean,
                // but if someone uses SELECT, check for non-empty results)
                if let Some(results) = json.get("results").and_then(|r| r.as_array()) {
                    if !results.is_empty() {
                        return Ok(());
                    }
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

/// Apply a single resource via POST /api/apply.
///
/// Wraps the resource declaration in a ResourceFile JSON envelope and posts
/// it to the server's apply endpoint. Returns the response.
pub async fn apply_resource(
    ctx: &TestContext,
    resource: serde_json::Value,
) -> Result<reqwest::Response, Box<dyn std::error::Error + Send + Sync>> {
    let resource_file = serde_json::json!({
        "resources": [resource]
    });
    http_post(ctx, "/api/apply", resource_file).await
}

/// Apply multiple resources via POST /api/apply.
pub async fn apply_resources(
    ctx: &TestContext,
    resources: Vec<serde_json::Value>,
) -> Result<reqwest::Response, Box<dyn std::error::Error + Send + Sync>> {
    let resource_file = serde_json::json!({
        "resources": resources
    });
    http_post(ctx, "/api/apply", resource_file).await
}

/// Apply a product resource and wait for it to appear in the RDF graph.
/// Returns Ok(()) when the product is projected, Err if it times out.
///
/// Idempotency across test runs (TC-353): if a prior run left a product of
/// this name on the cluster, the first apply returns 409 Conflict. This
/// helper detects that case, issues a `POST /api/delete`, waits for the
/// ProductDeleted event to propagate, then retries the apply once. Scenarios
/// that seed test-scoped products can therefore run cleanly regardless of
/// whether a prior run aborted before teardown.
pub async fn apply_product_and_wait(
    ctx: &TestContext,
    name: &str,
    version: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let product = serde_json::json!({
        "type": "product",
        "name": name,
        "version": version
    });

    let resp = apply_resource(ctx, product.clone()).await?;
    let status = resp.status();

    if status == reqwest::StatusCode::CONFLICT {
        // A lingering product from a prior run is blocking us. Delete it and
        // retry once. Failure to delete is not fatal — the retry will still
        // surface whatever error the server returns.
        tracing::warn!(
            product = name,
            "apply returned 409 Conflict — deleting lingering product and retrying"
        );

        let delete_resp = http_post(
            ctx,
            "/api/delete",
            serde_json::json!({ "product": name }),
        )
        .await?;
        if !delete_resp.status().is_success()
            && delete_resp.status().as_u16() != 202
        {
            let body = delete_resp.text().await.unwrap_or_default();
            return Err(format!(
                "apply product returned 409 and delete retry failed: {}",
                body
            )
            .into());
        }

        // Poll the graph until the product disappears (up to 15s) before
        // retrying the apply. This lets the ProductDeleted event propagate
        // through Raft + the RDF projector.
        let gone_ask = format!(
            "ASK {{ <https://picloud.local/products/{}> a <https://picloud.local/ontology#Product> }}",
            name
        );
        let gone_deadline = std::time::Instant::now() + Duration::from_secs(15);
        while std::time::Instant::now() < gone_deadline {
            match sparql_query(ctx, &gone_ask).await {
                Ok(body) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                        if json.get("boolean").and_then(|v| v.as_bool()) == Some(false) {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        let retry = apply_resource(ctx, product).await?;
        let retry_status = retry.status();
        if !retry_status.is_success() {
            let body = retry.text().await.unwrap_or_default();
            return Err(format!(
                "apply product failed after delete retry ({}): {}",
                retry_status, body
            )
            .into());
        }
    } else if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("apply product failed ({}): {}", status, body).into());
    }

    // Wait for the product to appear in the graph.
    let ask = format!(
        "ASK {{ <https://picloud.local/products/{}> a <https://picloud.local/ontology#Product> }}",
        name
    );
    wait_for_sparql(ctx, &ask, Duration::from_secs(15)).await
}
