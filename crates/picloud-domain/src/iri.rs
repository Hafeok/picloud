/// IRI Model
///
/// Every resource in PiCloud has a canonical IRI that is both its unique
/// identifier and its network location (ADR-029).
///
/// Scheme: https://{cluster_domain}/products/{product}/{type}/{name}
///
/// Examples:
///   https://picloud.local/products/photo-app/containers/api-server
///   https://picloud.local/products/photo-app/graph
///   https://picloud.local/products/photo-app/ontology
///   https://picloud.local/nodes/pi-node-01
///   https://picloud.local/schemas/events/ResourceReady/v1

use serde::{Deserialize, Serialize};
use std::fmt;
use url::Url;
use crate::error::{PiCloudError, Result};

/// The cluster domain — configurable, defaults to picloud.local
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClusterDomain(pub String);

impl Default for ClusterDomain {
    fn default() -> Self {
        Self("picloud.local".to_string())
    }
}

/// A fully-qualified, dereferenceable IRI for a platform resource.
/// IRIs are stable — they do not change when workloads reschedule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ResourceIri(pub String);

impl ResourceIri {
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        // Validate it parses as a URL
        Url::parse(&s).map_err(|_| PiCloudError::InvalidIri { iri: s.clone() })?;
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResourceIri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Constructs canonical IRIs for all resource types
pub struct IriBuilder {
    domain: ClusterDomain,
}

impl IriBuilder {
    pub fn new(domain: ClusterDomain) -> Self {
        Self { domain }
    }

    pub fn cluster_root(&self) -> ResourceIri {
        ResourceIri(format!("https://{}/", self.domain.0))
    }

    pub fn node(&self, node_name: &str) -> ResourceIri {
        ResourceIri(format!("https://{}/nodes/{}", self.domain.0, node_name))
    }

    pub fn product(&self, product_name: &str) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/products/{}",
            self.domain.0, product_name
        ))
    }

    pub fn resource(
        &self,
        product_name: &str,
        resource_type: &str,
        resource_name: &str,
    ) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/products/{}/{}/{}",
            self.domain.0, product_name, resource_type, resource_name
        ))
    }

    pub fn product_graph(&self, product_name: &str) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/products/{}/graph",
            self.domain.0, product_name
        ))
    }

    pub fn product_ontology(&self, product_name: &str) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/products/{}/ontology",
            self.domain.0, product_name
        ))
    }

    pub fn product_events(&self, product_name: &str) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/products/{}/events",
            self.domain.0, product_name
        ))
    }

    pub fn event_schema(&self, event_type: &str, version: u32) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/schemas/events/{}/v{}",
            self.domain.0, event_type, version
        ))
    }

    pub fn product_event_schema(
        &self,
        product_name: &str,
        event_type: &str,
        version: u32,
    ) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/products/{}/schemas/events/{}/v{}",
            self.domain.0, product_name, event_type, version
        ))
    }

    /// IRI for a product's config store: /products/{name}/config
    pub fn product_config(&self, product_name: &str) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/products/{}/config",
            self.domain.0, product_name
        ))
    }

    /// IRI for a single config entry: /products/{name}/config/{key}
    pub fn config_entry(&self, product_name: &str, key: &str) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/products/{}/config/{}",
            self.domain.0, product_name, key
        ))
    }

    /// IRI for a product's flags collection: /products/{name}/flags
    pub fn product_flags(&self, product_name: &str) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/products/{}/flags",
            self.domain.0, product_name
        ))
    }

    /// IRI for a single feature flag: /products/{name}/flags/{flag}
    pub fn feature_flag(&self, product_name: &str, flag_name: &str) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/products/{}/flags/{}",
            self.domain.0, product_name, flag_name
        ))
    }

    pub fn aggregate_stream(
        &self,
        product_name: &str,
        store_name: &str,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/products/{}/event-store/{}/{}/{}/events",
            self.domain.0, product_name, store_name, aggregate_type, aggregate_id
        ))
    }

    /// IRI for a group: /groups/{name} (ADR-037)
    pub fn group(&self, group_name: &str) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/groups/{}",
            self.domain.0, group_name
        ))
    }

    /// IRI for a platform-level inference/alert rule: /inference-rules/{name}
    pub fn inference_rule(&self, rule_name: &str) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/inference-rules/{}",
            self.domain.0, rule_name
        ))
    }

    /// IRI for a replay shadow graph: /replay/{replay_id} (ADR-035)
    pub fn replay_graph(&self, replay_id: &str) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/replay/{}",
            self.domain.0, replay_id
        ))
    }

    /// IRI for the cluster identity resource: /cluster/identity
    pub fn cluster_identity(&self) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/cluster/identity",
            self.domain.0
        ))
    }

    /// IRI for the OTel telemetry endpoint: /otel (ADR-045)
    pub fn otel_endpoint(&self) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/otel",
            self.domain.0
        ))
    }

    /// IRI for the telemetry store: /telemetry (ADR-046)
    pub fn telemetry(&self) -> ResourceIri {
        ResourceIri(format!(
            "https://{}/telemetry",
            self.domain.0
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builder() -> IriBuilder {
        IriBuilder::new(ClusterDomain::default())
    }

    #[test]
    fn cluster_domain_default() {
        assert_eq!(ClusterDomain::default().0, "picloud.local");
    }

    #[test]
    fn cluster_root() {
        let iri = builder().cluster_root();
        assert_eq!(iri.as_str(), "https://picloud.local/");
    }

    #[test]
    fn node_iri() {
        let iri = builder().node("pi-node-01");
        assert_eq!(iri.as_str(), "https://picloud.local/nodes/pi-node-01");
    }

    #[test]
    fn product_iri() {
        let iri = builder().product("photo-app");
        assert_eq!(iri.as_str(), "https://picloud.local/products/photo-app");
    }

    #[test]
    fn resource_iri() {
        let iri = builder().resource("photo-app", "containers", "api-server");
        assert_eq!(
            iri.as_str(),
            "https://picloud.local/products/photo-app/containers/api-server"
        );
    }

    #[test]
    fn product_graph_iri() {
        let iri = builder().product_graph("photo-app");
        assert_eq!(
            iri.as_str(),
            "https://picloud.local/products/photo-app/graph"
        );
    }

    #[test]
    fn product_events_iri() {
        let iri = builder().product_events("photo-app");
        assert_eq!(
            iri.as_str(),
            "https://picloud.local/products/photo-app/events"
        );
    }

    #[test]
    fn event_schema_iri() {
        let iri = builder().event_schema("ResourceReady", 1);
        assert_eq!(
            iri.as_str(),
            "https://picloud.local/schemas/events/ResourceReady/v1"
        );
    }

    #[test]
    fn product_event_schema_iri() {
        let iri = builder().product_event_schema("photo-app", "OrderPlaced", 2);
        assert_eq!(
            iri.as_str(),
            "https://picloud.local/products/photo-app/schemas/events/OrderPlaced/v2"
        );
    }

    #[test]
    fn resource_iri_new_valid_url() {
        let iri = ResourceIri::new("https://picloud.local/products/foo");
        assert!(iri.is_ok());
        assert_eq!(iri.unwrap().as_str(), "https://picloud.local/products/foo");
    }

    #[test]
    fn resource_iri_new_invalid_url() {
        let iri = ResourceIri::new("not a url");
        assert!(iri.is_err());
    }

    #[test]
    fn resource_iri_display() {
        let iri = ResourceIri("https://picloud.local/test".to_string());
        assert_eq!(format!("{}", iri), "https://picloud.local/test");
    }

    #[test]
    fn custom_domain() {
        let b = IriBuilder::new(ClusterDomain("my.cluster".to_string()));
        assert_eq!(b.cluster_root().as_str(), "https://my.cluster/");
        assert_eq!(b.node("n1").as_str(), "https://my.cluster/nodes/n1");
    }
}
