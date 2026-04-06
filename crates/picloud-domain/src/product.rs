/// Product Types
///
/// A Product is the top-level deployment unit in PiCloud (ADR-016).
/// It is versioned, hermetically sealed, and acts as an OIDC App Registration.
/// Products communicate only via events and SPARQL graphs — never directly.

use serde::{Deserialize, Serialize};
use crate::iri::ResourceIri;

/// The full declared specification of a Product — parsed from .picloud files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductSpec {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
}

/// Upgrade state for atomic cutover deployments (ADR-021)
/// New version reaches ResourceReady before old version is torn down.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeState {
    /// No upgrade in progress
    Stable,
    /// New version is being provisioned — old version still live
    Provisioning {
        from_version: String,
        to_version: String,
    },
    /// New version is fully ready — tearing down old version
    CuttingOver {
        from_version: String,
        to_version: String,
    },
    /// Upgrade failed — old version remains live, new resources cleaned up
    Failed {
        attempted_version: String,
        reason: String,
    },
}

/// Inter-product event subscription spec (ADR-022)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSubscriptionSpec {
    pub name: String,
    /// Source product name and version
    pub source: String,
    pub source_version: String,
    pub event_type: String,
    pub handler: String,
}

/// Aggregate definition within a product event store (ADR-032)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateSpec {
    pub aggregate_type: String,
    pub schema_file: String,
}

/// Event store spec declared in a .picloud file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventStoreSpec {
    pub name: String,
    pub product: String,
    pub aggregates: Vec<AggregateSpec>,
}

/// The cluster-level product registry entry — what the cluster graph knows about a product
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductRegistryEntry {
    pub product_iri: ResourceIri,
    pub name: String,
    pub version: String,
    pub sparql_endpoint: ResourceIri,
    pub ontology_iri: ResourceIri,
    pub events_endpoint: ResourceIri,
    pub emitted_event_types: Vec<String>,
    pub subscribed_event_types: Vec<String>,
}
