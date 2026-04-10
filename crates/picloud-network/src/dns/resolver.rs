//! DNS query resolver — cache-first, then SPARQL query against Oxigraph.

use std::net::Ipv4Addr;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug, warn};

use picloud_domain::error::Result;
use picloud_domain::traits::QueryResult;

use super::cache::{DnsCache, DnsRecord, SharedDnsCache};

/// Result of resolving a DNS query.
#[derive(Debug)]
pub enum ResolveResult {
    /// Records found with TTL.
    Records { records: Vec<DnsRecord>, ttl: u32 },
    /// Name does not exist in this zone.
    NxDomain,
    /// Record type not implemented.
    NotImplemented,
}

/// Callback type for executing SPARQL queries against the RDF graph.
/// Injected from the composition root to avoid cross-slice dependencies.
pub type SparqlQueryFn = Arc<dyn Fn(&str) -> Result<QueryResult> + Send + Sync>;

/// Authoritative DNS resolver for the cluster's tenant domain.
pub struct DnsResolver {
    domain: String,
    cache: SharedDnsCache,
    sparql_query: Option<SparqlQueryFn>,
}

impl DnsResolver {
    pub fn new(domain: String) -> Self {
        Self {
            domain,
            cache: Arc::new(RwLock::new(DnsCache::new())),
            sparql_query: None,
        }
    }

    /// Set the SPARQL query function (injected from composition root).
    pub fn with_sparql_query(mut self, query_fn: SparqlQueryFn) -> Self {
        self.sparql_query = Some(query_fn);
        self
    }

    /// Get a reference to the shared cache (for event-driven invalidation).
    pub fn cache(&self) -> SharedDnsCache {
        self.cache.clone()
    }

    /// Check if this resolver is authoritative for the given name.
    pub fn is_authoritative(&self, name: &str) -> bool {
        let lower = name.to_lowercase();
        lower == self.domain || lower.ends_with(&format!(".{}", self.domain))
    }

    /// Resolve a DNS query.
    pub async fn resolve(&self, name: &str, record_type: &str) -> ResolveResult {
        // Only answer for our domain
        if !self.is_authoritative(name) {
            debug!(name = %name, "Not authoritative, returning NxDomain");
            return ResolveResult::NxDomain;
        }

        let rtype = record_type.to_uppercase();

        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(records) = cache.get(&rtype, name) {
                return ResolveResult::Records { records, ttl: 30 };
            }
        }

        // Cache miss — query the RDF graph
        let records = match rtype.as_str() {
            "A" => self.query_a(name).await,
            "SRV" => self.query_srv(name).await,
            "TXT" => self.query_txt(name).await,
            "PTR" => self.query_ptr(name).await,
            _ => return ResolveResult::NotImplemented,
        };

        match records {
            Ok(records) if records.is_empty() => ResolveResult::NxDomain,
            Ok(records) => {
                // Cache the result
                let mut cache = self.cache.write().await;
                cache.insert(&rtype, name, records.clone());
                ResolveResult::Records { records, ttl: 30 }
            }
            Err(e) => {
                warn!(name = %name, error = %e, "DNS query failed, returning NxDomain");
                ResolveResult::NxDomain
            }
        }
    }

    async fn query_a(&self, name: &str) -> Result<Vec<DnsRecord>> {
        let Some(ref query_fn) = self.sparql_query else {
            return Ok(Vec::new());
        };

        let sparql = super::records::A_RECORD_QUERY
            .replace("$hostname", &format!("\"{}\"", name));
        let result = query_fn(&sparql)?;

        let mut records = Vec::new();
        for binding in &result.bindings {
            if let Some(addr_str) = binding.get("address").and_then(|v| v.as_str()) {
                // Parse IP from address (may include port like "192.168.1.10:8443")
                let ip_str = addr_str.split(':').next().unwrap_or(addr_str);
                if let Ok(addr) = ip_str.parse::<Ipv4Addr>() {
                    records.push(DnsRecord::A { address: addr });
                }
            }
        }
        Ok(records)
    }

    async fn query_srv(&self, name: &str) -> Result<Vec<DnsRecord>> {
        let Some(ref query_fn) = self.sparql_query else {
            return Ok(Vec::new());
        };

        let Some((service_type, _proto, product)) =
            super::records::parse_srv_name(name, &self.domain)
        else {
            return Ok(Vec::new());
        };

        let sparql = super::records::SRV_RECORD_QUERY
            .replace("$service_type", &format!("\"{}\"", service_type))
            .replace("$product", &format!("\"{}\"", product));
        let result = query_fn(&sparql)?;

        let mut records = Vec::new();
        for binding in &result.bindings {
            let target = binding.get("target").and_then(|v| v.as_str()).unwrap_or_default();
            let port = binding.get("port").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
            let priority = binding.get("priority").and_then(|v| v.as_u64()).unwrap_or(10) as u16;
            let weight = binding.get("weight").and_then(|v| v.as_u64()).unwrap_or(10) as u16;
            records.push(DnsRecord::Srv {
                target: target.to_string(),
                port,
                priority,
                weight,
            });
        }
        Ok(records)
    }

    async fn query_txt(&self, name: &str) -> Result<Vec<DnsRecord>> {
        let Some(ref query_fn) = self.sparql_query else {
            return Ok(Vec::new());
        };

        let sparql = super::records::TXT_RECORD_QUERY
            .replace("$hostname", &format!("\"{}\"", name));
        let result = query_fn(&sparql)?;

        let mut records = Vec::new();
        for binding in &result.bindings {
            let key = binding.get("key").and_then(|v| v.as_str()).unwrap_or_default();
            let value = binding.get("value").and_then(|v| v.as_str()).unwrap_or_default();
            records.push(DnsRecord::Txt {
                text: super::records::format_product_txt(key, value),
            });
        }
        Ok(records)
    }

    async fn query_ptr(&self, name: &str) -> Result<Vec<DnsRecord>> {
        let Some(ref query_fn) = self.sparql_query else {
            return Ok(Vec::new());
        };

        let Some(address) = super::records::parse_ptr_name(name) else {
            return Ok(Vec::new());
        };

        let sparql = super::records::PTR_RECORD_QUERY
            .replace("$address", &format!("\"{}\"", address));
        let result = query_fn(&sparql)?;

        let mut records = Vec::new();
        for binding in &result.bindings {
            if let Some(hostname) = binding.get("hostname").and_then(|v| v.as_str()) {
                records.push(DnsRecord::Ptr { name: hostname.to_string() });
            }
        }
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_authoritative_for_own_domain() {
        let resolver = DnsResolver::new("picloud.local".to_string());
        assert!(resolver.is_authoritative("picloud.local"));
        assert!(resolver.is_authoritative("photos.picloud.local"));
        assert!(resolver.is_authoritative("_sparql._tcp.app.picloud.local"));
        assert!(!resolver.is_authoritative("example.com"));
        assert!(!resolver.is_authoritative("picloud.localx"));
    }

    #[tokio::test]
    async fn resolve_non_authoritative_returns_nxdomain() {
        let resolver = DnsResolver::new("picloud.local".to_string());
        let result = resolver.resolve("example.com", "A").await;
        assert!(matches!(result, ResolveResult::NxDomain));
    }

    #[tokio::test]
    async fn resolve_unsupported_type_returns_not_implemented() {
        let resolver = DnsResolver::new("picloud.local".to_string());
        let result = resolver.resolve("test.picloud.local", "AAAA").await;
        assert!(matches!(result, ResolveResult::NotImplemented));
    }
}
