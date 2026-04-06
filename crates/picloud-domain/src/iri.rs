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
}
