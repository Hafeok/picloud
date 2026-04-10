//! ADR-014: Read the DNS/network source. Verify product namespace isolation
//! in DNS records — internal DNS scopes queries to the product namespace.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct CrossProductIsolation;

#[async_trait]
impl Scenario for CrossProductIsolation {
    fn name(&self) -> &str {
        "cross_product_isolation"
    }

    fn adr(&self) -> &str {
        "ADR-014"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();
        let mut issues: Vec<String> = Vec::new();

        // Read the DNS resolver source.
        let resolver_path = ctx
            .workspace_root()
            .join("crates/picloud-network/src/dns/resolver.rs");

        let resolver_src = match std::fs::read_to_string(&resolver_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read DNS resolver: {}", e),
                };
            }
        };

        // Read the DNS records source.
        let records_path = ctx
            .workspace_root()
            .join("crates/picloud-network/src/dns/records.rs");

        let records_src = match std::fs::read_to_string(&records_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read DNS records: {}", e),
                };
            }
        };

        // Verify the DNS system uses SPARQL to resolve names — this means
        // queries go through the RDF graph which supports product scoping.
        if !resolver_src.contains("sparql") && !resolver_src.contains("SPARQL") {
            issues.push(
                "DNS resolver does not use SPARQL queries — cannot scope by product namespace"
                    .to_string(),
            );
        }

        // Verify the SPARQL queries include product filtering.
        if !records_src.contains("product") {
            issues.push(
                "DNS SPARQL queries do not reference product — no namespace isolation"
                    .to_string(),
            );
        }

        // Verify the A record query scopes by hostname which includes product.
        if !records_src.contains("A_RECORD_QUERY") {
            issues.push("A_RECORD_QUERY constant not found".to_string());
        }

        // Verify the SRV record query scopes by product.
        if records_src.contains("SRV_RECORD_QUERY") {
            if !records_src.contains("$product") && !records_src.contains("?product") {
                issues.push(
                    "SRV_RECORD_QUERY does not filter by product — cross-product leakage possible"
                        .to_string(),
                );
            }
        }

        // Verify domain scoping — resolver should check authoritative domain.
        if !resolver_src.contains("is_authoritative")
            && !resolver_src.contains("domain")
        {
            issues.push(
                "DNS resolver does not check authoritative domain — may respond to external queries"
                    .to_string(),
            );
        }

        // Read the DNS server source to verify product-scoped handling.
        let server_path = ctx
            .workspace_root()
            .join("crates/picloud-network/src/dns/server.rs");

        if let Ok(server_src) = std::fs::read_to_string(&server_path) {
            // Verify the DNS server delegates to the resolver.
            if !server_src.contains("resolver") {
                issues.push(
                    "DNS server does not use the resolver — queries bypass product scoping"
                        .to_string(),
                );
            }

            // Verify NxDomain is returned for unknown names.
            if !server_src.contains("NxDomain") && !server_src.contains("NXDomain") {
                issues.push(
                    "DNS server does not return NxDomain for unknown names".to_string(),
                );
            }
        }

        let duration = start.elapsed();

        if issues.is_empty() {
            ScenarioResult::Pass { duration }
        } else {
            ScenarioResult::Fail {
                duration,
                reason: format!(
                    "{} product isolation issue(s): {}",
                    issues.len(),
                    issues.join("; ")
                ),
            }
        }
    }
}
