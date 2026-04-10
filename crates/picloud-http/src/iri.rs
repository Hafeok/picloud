//! IRI-based content negotiation and routing (ADR-048, ADR-029).
//!
//! Every resource in PiCloud has a dereferenceable IRI. This module
//! maps URL paths to IRI routes and negotiates the response format
//! based on the Accept header.

use serde::{Deserialize, Serialize};

/// Supported content types for content negotiation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentType {
    /// text/turtle — RDF Turtle format
    Turtle,
    /// application/ld+json — JSON-LD
    JsonLd,
    /// application/json — plain JSON
    Json,
    /// text/html — HTML representation
    Html,
}

impl ContentType {
    /// Parse the Accept header to determine the preferred content type.
    pub fn from_accept(accept: &str) -> Self {
        let lower = accept.to_lowercase();
        if lower.contains("text/turtle") || lower.contains("application/x-turtle") {
            ContentType::Turtle
        } else if lower.contains("application/ld+json") {
            ContentType::JsonLd
        } else if lower.contains("text/html") {
            ContentType::Html
        } else {
            // Default to JSON for application/json or anything else
            ContentType::Json
        }
    }

    /// Return the MIME type string.
    pub fn mime_type(&self) -> &'static str {
        match self {
            ContentType::Turtle => "text/turtle; charset=utf-8",
            ContentType::JsonLd => "application/ld+json; charset=utf-8",
            ContentType::Json => "application/json; charset=utf-8",
            ContentType::Html => "text/html; charset=utf-8",
        }
    }
}

/// IRI routes — the set of URL paths the HTTP server handles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IriRoute {
    /// GET / — cluster root
    ClusterRoot,
    /// GET /nodes/{name} — a specific node
    Node { name: String },
    /// GET /products/{product} — a product
    Product { product: String },
    /// GET /products/{product}/graph — product's RDF graph (SPARQL endpoint)
    ProductGraph { product: String },
    /// GET /products/{product}/ontology — product's ontology
    ProductOntology { product: String },
    /// GET /products/{product}/events — product's event stream
    ProductEvents { product: String },
    /// GET /products/{product}/config — product's configuration
    ProductConfig { product: String },
    /// GET /products/{product}/config/{key} — a specific config entry
    ConfigEntry { product: String, key: String },
    /// GET /products/{product}/flags — product's feature flags
    ProductFlags { product: String },
    /// GET /products/{product}/flags/{name} — a specific feature flag
    FeatureFlag { product: String, name: String },
    /// GET /products/{product}/event-store/{store}/{agg_type}/{agg_id}/events
    AggregateStream {
        product: String,
        store: String,
        agg_type: String,
        agg_id: String,
    },
    /// GET /schemas/events/{type}/{version} — event schema
    EventSchema { event_type: String, version: String },
    /// GET /products/{product}/{type}/{name} — generic resource
    Resource {
        product: String,
        resource_type: String,
        name: String,
    },
    /// No matching route
    NotFound,
}

/// Route a URL path to an IriRoute.
pub fn route_iri(path: &str) -> IriRoute {
    let segments: Vec<&str> = path
        .trim_start_matches('/')
        .trim_end_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    match segments.as_slice() {
        [] => IriRoute::ClusterRoot,
        ["nodes", name] => IriRoute::Node {
            name: name.to_string(),
        },
        ["products", product] => IriRoute::Product {
            product: product.to_string(),
        },
        ["products", product, "graph"] => IriRoute::ProductGraph {
            product: product.to_string(),
        },
        ["products", product, "ontology"] => IriRoute::ProductOntology {
            product: product.to_string(),
        },
        ["products", product, "events"] => IriRoute::ProductEvents {
            product: product.to_string(),
        },
        ["products", product, "config"] => IriRoute::ProductConfig {
            product: product.to_string(),
        },
        ["products", product, "config", key] => IriRoute::ConfigEntry {
            product: product.to_string(),
            key: key.to_string(),
        },
        ["products", product, "flags"] => IriRoute::ProductFlags {
            product: product.to_string(),
        },
        ["products", product, "flags", name] => IriRoute::FeatureFlag {
            product: product.to_string(),
            name: name.to_string(),
        },
        ["products", product, "event-store", store, agg_type, agg_id, "events"] => {
            IriRoute::AggregateStream {
                product: product.to_string(),
                store: store.to_string(),
                agg_type: agg_type.to_string(),
                agg_id: agg_id.to_string(),
            }
        }
        ["schemas", "events", event_type, version] => IriRoute::EventSchema {
            event_type: event_type.to_string(),
            version: version.to_string(),
        },
        ["products", product, resource_type, name] => IriRoute::Resource {
            product: product.to_string(),
            resource_type: resource_type.to_string(),
            name: name.to_string(),
        },
        _ => IriRoute::NotFound,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_cluster_root() {
        assert_eq!(route_iri("/"), IriRoute::ClusterRoot);
    }

    #[test]
    fn route_node() {
        assert_eq!(
            route_iri("/nodes/pi-01"),
            IriRoute::Node {
                name: "pi-01".to_string()
            }
        );
    }

    #[test]
    fn route_product() {
        assert_eq!(
            route_iri("/products/photo-app"),
            IriRoute::Product {
                product: "photo-app".to_string()
            }
        );
    }

    #[test]
    fn route_product_graph() {
        assert_eq!(
            route_iri("/products/photo-app/graph"),
            IriRoute::ProductGraph {
                product: "photo-app".to_string()
            }
        );
    }

    #[test]
    fn route_feature_flag() {
        assert_eq!(
            route_iri("/products/photo-app/flags/dark-mode"),
            IriRoute::FeatureFlag {
                product: "photo-app".to_string(),
                name: "dark-mode".to_string()
            }
        );
    }

    #[test]
    fn route_aggregate_stream() {
        assert_eq!(
            route_iri("/products/photo-app/event-store/photos/Album/album-1/events"),
            IriRoute::AggregateStream {
                product: "photo-app".to_string(),
                store: "photos".to_string(),
                agg_type: "Album".to_string(),
                agg_id: "album-1".to_string(),
            }
        );
    }

    #[test]
    fn route_event_schema() {
        assert_eq!(
            route_iri("/schemas/events/ResourceReady/v1"),
            IriRoute::EventSchema {
                event_type: "ResourceReady".to_string(),
                version: "v1".to_string()
            }
        );
    }

    #[test]
    fn route_generic_resource() {
        assert_eq!(
            route_iri("/products/photo-app/containers/api-server"),
            IriRoute::Resource {
                product: "photo-app".to_string(),
                resource_type: "containers".to_string(),
                name: "api-server".to_string()
            }
        );
    }

    #[test]
    fn route_not_found() {
        assert_eq!(route_iri("/random/path/that/doesnt/match/anything"), IriRoute::NotFound);
    }

    #[test]
    fn content_type_from_accept_turtle() {
        assert_eq!(ContentType::from_accept("text/turtle"), ContentType::Turtle);
    }

    #[test]
    fn content_type_from_accept_json_ld() {
        assert_eq!(
            ContentType::from_accept("application/ld+json"),
            ContentType::JsonLd
        );
    }

    #[test]
    fn content_type_from_accept_json() {
        assert_eq!(
            ContentType::from_accept("application/json"),
            ContentType::Json
        );
    }

    #[test]
    fn content_type_from_accept_html() {
        assert_eq!(ContentType::from_accept("text/html"), ContentType::Html);
    }

    #[test]
    fn content_type_default_is_json() {
        assert_eq!(ContentType::from_accept("*/*"), ContentType::Json);
    }

    #[test]
    fn mime_type_strings() {
        assert!(ContentType::Turtle.mime_type().contains("text/turtle"));
        assert!(ContentType::Json.mime_type().contains("application/json"));
    }
}
