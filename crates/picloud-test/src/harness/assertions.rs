use std::net::IpAddr;

use hickory_resolver::config::{NameServerConfig, ResolverConfig};
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::proto::xfer::Protocol;
use hickory_resolver::TokioResolver;

use crate::harness::runner::TestContext;

/// POST a SPARQL query to the cluster and return the raw response body.
pub async fn sparql_query(
    ctx: &TestContext,
    query: &str,
) -> Result<String, Box<dyn std::error::Error>> {
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
) -> Result<reqwest::Response, Box<dyn std::error::Error>> {
    let url = format!("{}{}", ctx.config.base_url(), path);
    let resp = ctx.http_client.get(&url).send().await?;
    Ok(resp)
}

/// POST to a cluster HTTP endpoint with a JSON body.
pub async fn http_post(
    ctx: &TestContext,
    path: &str,
    body: serde_json::Value,
) -> Result<reqwest::Response, Box<dyn std::error::Error>> {
    let url = format!("{}{}", ctx.config.base_url(), path);
    let resp = ctx.http_client.post(&url).json(&body).send().await?;
    Ok(resp)
}

/// Resolve a hostname using the cluster's DNS (first node IP, port 53).
pub async fn dns_lookup(
    ctx: &TestContext,
    hostname: &str,
) -> Result<Vec<IpAddr>, Box<dyn std::error::Error>> {
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
) -> Result<(), Box<dyn std::error::Error>> {
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

/// Assert that an HTTP GET to the given path returns the expected status code.
pub async fn assert_http_status(
    ctx: &TestContext,
    path: &str,
    expected_status: u16,
) -> Result<(), Box<dyn std::error::Error>> {
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
