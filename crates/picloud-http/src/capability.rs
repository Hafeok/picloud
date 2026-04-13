/// Capability Resolution (ADR-055)
///
/// Resolves capabilities to their implementing Products and routes events
/// between Products through the platform event bus. Uses SPARQL queries
/// against the RDF graph to find implementors and consumers.

use async_trait::async_trait;
use std::sync::Arc;
use tracing::debug;

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{
    CapabilityResolution, CapabilityResolver, CapabilityStatus, EventLog, StateProjector,
};

/// Implementation of `CapabilityResolver` that queries the RDF graph for
/// capability metadata and routes events through the event log.
pub struct CapabilityResolverImpl {
    projector: Arc<dyn StateProjector>,
    event_log: Arc<dyn EventLog>,
    cluster_domain: ClusterDomain,
}

impl CapabilityResolverImpl {
    pub fn new(
        projector: Arc<dyn StateProjector>,
        event_log: Arc<dyn EventLog>,
        cluster_domain: ClusterDomain,
    ) -> Self {
        Self {
            projector,
            event_log,
            cluster_domain,
        }
    }
}

/// Compare two semver-like version strings. Returns true if `offered` >= `required`.
/// Compares major.minor.patch numerically, treating missing components as 0.
fn version_satisfies(offered: &str, required: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let o = parse(offered);
    let r = parse(required);
    // Compare component by component, padding with 0
    for i in 0..std::cmp::max(o.len(), r.len()) {
        let ov = o.get(i).copied().unwrap_or(0);
        let rv = r.get(i).copied().unwrap_or(0);
        if ov > rv {
            return true;
        }
        if ov < rv {
            return false;
        }
    }
    true // equal versions satisfy
}

/// Compare two version strings for ordering. Returns Ordering.
fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |v: &str| -> Vec<u64> {
        v.split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let va = parse(a);
    let vb = parse(b);
    let max_len = std::cmp::max(va.len(), vb.len());
    for i in 0..max_len {
        let a_part = va.get(i).copied().unwrap_or(0);
        let b_part = vb.get(i).copied().unwrap_or(0);
        match a_part.cmp(&b_part) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

#[async_trait]
impl CapabilityResolver for CapabilityResolverImpl {
    async fn resolve_implementor(
        &self,
        capability_name: &str,
        min_version: &str,
    ) -> Result<CapabilityResolution> {
        // SPARQL: find all products that implement capabilities with this name,
        // along with the capability version.
        let sparql = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
            SELECT ?capability ?version ?product WHERE {{
                ?capability rdf:type picloud:Capability .
                ?capability picloud:name "{name}" .
                ?capability picloud:version ?version .
                ?capability picloud:implementedBy ?product .
            }}
            "#,
            name = capability_name,
        );

        let result = self.projector.query(&sparql).await?;

        // Find the highest version implementor that satisfies min_version
        let mut best: Option<CapabilityResolution> = None;

        for binding in &result.bindings {
            let version = binding["version"]
                .as_str()
                .or_else(|| binding["version"]["value"].as_str())
                .unwrap_or_default();
            let product_iri = binding["product"]
                .as_str()
                .or_else(|| binding["product"]["value"].as_str())
                .unwrap_or_default();

            if !version_satisfies(version, min_version) {
                continue;
            }

            // Extract product name from IRI: .../products/<name>
            let product_name = product_iri
                .rsplit("/products/")
                .next()
                .and_then(|s| s.split('/').next())
                .unwrap_or(product_iri)
                .to_string();

            let is_better = match &best {
                None => true,
                Some(current) => {
                    version_cmp(version, &current.implementor_version)
                        == std::cmp::Ordering::Greater
                }
            };

            if is_better {
                best = Some(CapabilityResolution {
                    capability_name: capability_name.to_string(),
                    implementor_product: product_name,
                    implementor_version: version.to_string(),
                });
            }
        }

        best.ok_or_else(|| PiCloudError::CapabilityUnfulfilled {
            capability: capability_name.to_string(),
            min_version: min_version.to_string(),
        })
    }

    async fn route_capability_event(
        &self,
        capability_name: &str,
        event: &EventEnvelope,
    ) -> Result<()> {
        // Determine the min_version from the event's source product or default to "1.0.0"
        let min_version = event
            .payload
            .get("minVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("1.0.0");

        let resolution = self.resolve_implementor(capability_name, min_version).await?;

        debug!(
            capability = capability_name,
            implementor = %resolution.implementor_product,
            version = %resolution.implementor_version,
            "Routing capability event to implementor"
        );

        // Create a new event scoped to the implementor's product
        let iri_builder = IriBuilder::new(self.cluster_domain.clone());
        let routed_event = EventEnvelope::new(
            iri_builder.event_schema("CapabilityEventRouted", 1),
            "CapabilityEventRouted",
            event.source.clone(),
            Some(resolution.implementor_product.clone()),
            event.correlation_id,
            serde_json::json!({
                "capability": capability_name,
                "implementor_product": resolution.implementor_product,
                "implementor_version": resolution.implementor_version,
                "original_event_type": event.event_type,
                "original_event_id": event.id.to_string(),
                "original_payload": event.payload,
            }),
        );

        self.event_log.append(routed_event).await.map_err(|e| {
            PiCloudError::CapabilityRoutingFailed {
                capability: capability_name.to_string(),
                reason: format!("failed to append routed event: {e}"),
            }
        })?;

        debug!(
            capability = capability_name,
            target = %resolution.implementor_product,
            "Capability event routed successfully"
        );

        Ok(())
    }

    async fn list_capabilities(&self) -> Result<Vec<CapabilityStatus>> {
        let sparql = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
            SELECT ?capability ?name ?version ?status WHERE {
                ?capability rdf:type picloud:Capability .
                ?capability picloud:name ?name .
                ?capability picloud:version ?version .
                OPTIONAL { ?capability picloud:status ?status }
            }
        "#;

        let result = self.projector.query(sparql).await?;
        let mut capabilities = Vec::new();

        for binding in &result.bindings {
            let capability_iri_str = binding["capability"]
                .as_str()
                .or_else(|| binding["capability"]["value"].as_str())
                .unwrap_or_default();
            let name = binding["name"]
                .as_str()
                .or_else(|| binding["name"]["value"].as_str())
                .unwrap_or_default();
            let version = binding["version"]
                .as_str()
                .or_else(|| binding["version"]["value"].as_str())
                .unwrap_or_default();
            let status = binding["status"]
                .as_str()
                .or_else(|| binding["status"]["value"].as_str())
                .unwrap_or("declared");

            // Query implementors for this capability
            let impl_sparql = format!(
                r#"
                PREFIX picloud: <https://picloud.local/ontology#>
                SELECT ?product WHERE {{
                    <{iri}> picloud:implementedBy ?product .
                }}
                "#,
                iri = capability_iri_str,
            );
            let impl_result = self.projector.query(&impl_sparql).await.unwrap_or(
                picloud_domain::traits::QueryResult {
                    bindings: Vec::new(),
                },
            );
            let implementors: Vec<String> = impl_result
                .bindings
                .iter()
                .filter_map(|b| {
                    b["product"]
                        .as_str()
                        .or_else(|| b["product"]["value"].as_str())
                })
                .map(|s| s.to_string())
                .collect();

            let fulfilled = !implementors.is_empty() && (status == "ready" || status.ends_with("Ready"));

            // Query consumers (products that require this capability)
            let consumer_sparql = format!(
                r#"
                PREFIX picloud: <https://picloud.local/ontology#>
                SELECT ?product WHERE {{
                    <{iri}> picloud:consumedBy ?product .
                }}
                "#,
                iri = capability_iri_str,
            );
            let consumer_result = self.projector.query(&consumer_sparql).await.unwrap_or(
                picloud_domain::traits::QueryResult {
                    bindings: Vec::new(),
                },
            );
            let consumers: Vec<String> = consumer_result
                .bindings
                .iter()
                .filter_map(|b| {
                    b["product"]
                        .as_str()
                        .or_else(|| b["product"]["value"].as_str())
                })
                .map(|s| s.to_string())
                .collect();

            capabilities.push(CapabilityStatus {
                capability_iri: ResourceIri(capability_iri_str.to_string()),
                name: name.to_string(),
                version: version.to_string(),
                fulfilled,
                implementors,
                consumers,
            });
        }

        Ok(capabilities)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_satisfies_basic() {
        assert!(version_satisfies("1.0.0", "1.0.0"));
        assert!(version_satisfies("2.0.0", "1.0.0"));
        assert!(version_satisfies("1.1.0", "1.0.0"));
        assert!(version_satisfies("1.0.1", "1.0.0"));
        assert!(!version_satisfies("0.9.0", "1.0.0"));
        assert!(!version_satisfies("1.0.0", "2.0.0"));
    }

    #[test]
    fn version_satisfies_partial() {
        assert!(version_satisfies("2", "1.0.0"));
        assert!(version_satisfies("1.0.0", "1"));
        assert!(!version_satisfies("0", "1.0.0"));
    }

    #[test]
    fn version_cmp_ordering() {
        assert_eq!(version_cmp("1.0.0", "1.0.0"), std::cmp::Ordering::Equal);
        assert_eq!(version_cmp("2.0.0", "1.0.0"), std::cmp::Ordering::Greater);
        assert_eq!(version_cmp("1.0.0", "2.0.0"), std::cmp::Ordering::Less);
        assert_eq!(version_cmp("1.2.0", "1.1.0"), std::cmp::Ordering::Greater);
    }
}
