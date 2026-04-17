//! DataProductProjector — runs SPARQL CONSTRUCT projections and manages
//! data product named graphs (ADR-056).
//!
//! Depends only on picloud-domain. Uses the same Oxigraph store as the
//! main `OxigraphProjector` so that CONSTRUCT queries can read from
//! product graphs and write into data product graphs.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use oxigraph::model::{NamedNode, NamedNodeRef, QuadRef};
use oxigraph::sparql::QueryResults;
use oxigraph::store::{StorageError, Store};
use tracing::info;

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::iri::ResourceIri;
use picloud_domain::traits::{DataProductProjector, DataProductRefreshResult, QueryResult};

/// Oxigraph-backed implementation of `DataProductProjector`.
///
/// Shares the same `Store` as the main `OxigraphProjector` so that
/// CONSTRUCT queries can read from any graph in the store.
pub struct OxigraphDataProductProjector {
    store: Arc<Store>,
}

impl OxigraphDataProductProjector {
    /// Create a new data product projector backed by the given store.
    ///
    /// The store should be the same instance used by the `OxigraphProjector`
    /// so that source graphs (product event projections) are visible.
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }

    /// Derive the named graph IRI for a data product from its resource IRI.
    ///
    /// Convention: `<data_product_iri>/graph`
    fn data_product_graph(data_product_iri: &ResourceIri) -> String {
        format!("{}/graph", data_product_iri.as_str())
    }

    /// Execute a SPARQL SELECT/ASK and convert results to JSON bindings.
    fn execute_query(&self, sparql: &str) -> Result<QueryResult> {
        let results = self
            .store
            .query(sparql)
            .map_err(|e| PiCloudError::Internal(format!("SPARQL query failed: {e}")))?;
        match results {
            QueryResults::Solutions(solutions) => {
                let variables: Vec<String> =
                    solutions.variables().iter().map(|v| v.as_str().to_owned()).collect();
                let mut bindings = Vec::new();
                for solution in solutions {
                    let solution = solution
                        .map_err(|e| PiCloudError::Internal(format!("solution error: {e}")))?;
                    let mut row = serde_json::Map::new();
                    for var in &variables {
                        if let Some(term) = solution.get(var.as_str()) {
                            row.insert(var.clone(), term_to_json(term));
                        }
                    }
                    bindings.push(serde_json::Value::Object(row));
                }
                Ok(QueryResult { bindings })
            }
            QueryResults::Boolean(value) => {
                let binding = serde_json::json!({ "result": value });
                Ok(QueryResult {
                    bindings: vec![binding],
                })
            }
            QueryResults::Graph(_) => Ok(QueryResult {
                bindings: Vec::new(),
            }),
        }
    }
}

#[async_trait]
impl DataProductProjector for OxigraphDataProductProjector {
    async fn refresh_projection(
        &self,
        data_product_iri: &ResourceIri,
        construct_query: &str,
        _source_graph_iri: &ResourceIri,
    ) -> Result<DataProductRefreshResult> {
        let start = Instant::now();
        let dp_graph = Self::data_product_graph(data_product_iri);

        // --- Step 1: Execute the CONSTRUCT into an in-memory triple list ---
        // The CONSTRUCT query itself should reference the source graph
        // (e.g. via GRAPH <source_graph_iri> { ... }) in its body.
        // Reading is an immutable operation and completes independently of
        // the atomic swap that follows.
        let results = self
            .store
            .query(construct_query)
            .map_err(|e| PiCloudError::Internal(format!("CONSTRUCT query failed: {e}")))?;

        let triples = match results {
            QueryResults::Graph(graph) => {
                let mut collected = Vec::new();
                for triple in graph {
                    let triple = triple.map_err(|e| {
                        PiCloudError::Internal(format!("CONSTRUCT result error: {e}"))
                    })?;
                    collected.push(triple);
                }
                collected
            }
            _ => {
                return Err(PiCloudError::ResourceValidationFailed {
                    reason: "refresh_projection requires a CONSTRUCT query".to_string(),
                });
            }
        };

        let triple_count = triples.len() as u64;

        // Ensure the named graph exists before the transaction opens.
        self.store
            .insert_named_graph(
                NamedNodeRef::new(&dp_graph)
                    .map_err(|e| PiCloudError::Internal(format!("invalid graph IRI: {e}")))?,
            )
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;

        // --- Step 2: Atomic swap via an Oxigraph transaction ---
        // Oxigraph transactions provide "repeatable read" isolation: no
        // concurrent SPARQL query ever observes a partial state (some old
        // triples mixed with some new triples). During the transaction
        // in-flight reads complete against the old graph; after commit
        // new reads see the new graph. (TC-200: atomic swap.)
        let dp_graph_node = NamedNode::new(&dp_graph)
            .map_err(|e| PiCloudError::Internal(format!("invalid graph IRI: {e}")))?;
        self.store
            .transaction::<_, StorageError>(|mut t| {
                // 1. Clear the live graph inside the transaction — readers
                //    outside the transaction still see the old triples until
                //    commit.
                t.clear_graph(NamedNodeRef::from(&dp_graph_node))?;

                // 2. Insert the new CONSTRUCT result triples into the live graph.
                for triple in &triples {
                    t.insert(QuadRef::new(
                        &triple.subject,
                        &triple.predicate,
                        &triple.object,
                        &dp_graph_node,
                    ))?;
                }

                Ok(())
            })
            .map_err(|e| PiCloudError::Internal(format!("atomic swap failed: {e}")))?;

        let duration_ms = start.elapsed().as_millis() as u64;
        info!(
            data_product = %data_product_iri.as_str(),
            triple_count,
            duration_ms,
            "Data product projection refreshed (atomic swap)"
        );

        Ok(DataProductRefreshResult {
            triple_count,
            duration_ms,
        })
    }

    async fn query_data_product(
        &self,
        data_product_iri: &ResourceIri,
        sparql: &str,
    ) -> Result<QueryResult> {
        let dp_graph = Self::data_product_graph(data_product_iri);
        let wrapped = format!(
            "SELECT * WHERE {{ GRAPH <{dp_graph}> {{ {sparql} }} }}"
        );
        self.execute_query(&wrapped)
    }
}

/// Convert an RDF term to a JSON value (mirrors the helper in implementation.rs).
fn term_to_json(term: &oxigraph::model::Term) -> serde_json::Value {
    match term {
        oxigraph::model::Term::NamedNode(n) => serde_json::Value::String(n.as_str().to_owned()),
        oxigraph::model::Term::BlankNode(b) => {
            serde_json::Value::String(format!("_:{}", b.as_str()))
        }
        oxigraph::model::Term::Literal(lit) => serde_json::Value::String(lit.value().to_owned()),
        _ => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigraph::model::{Literal, NamedNode as NN, QuadRef as QR};
    use picloud_domain::iri::{ClusterDomain, IriBuilder};

    fn test_store_with_triples() -> Store {
        let store = Store::new().unwrap();
        let iri_builder = IriBuilder::new(ClusterDomain::default());
        let product_graph_iri = format!("{}/graph", iri_builder.product("myapp").as_str());

        // Insert named graph
        store
            .insert_named_graph(NamedNodeRef::new(&product_graph_iri).unwrap())
            .unwrap();

        // Insert some triples into the product graph
        let subj = NN::new("https://picloud.local/products/myapp/things/thing1").unwrap();
        let pred = NN::new("https://picloud.local/ontology#name").unwrap();
        let obj = Literal::new_simple_literal("Thing One");
        let g = NN::new(&product_graph_iri).unwrap();
        store.insert(QR::new(&subj, &pred, &obj, &g)).unwrap();

        let pred2 = NN::new("https://picloud.local/ontology#status").unwrap();
        let obj2 = Literal::new_simple_literal("active");
        store.insert(QR::new(&subj, &pred2, &obj2, &g)).unwrap();

        store
    }

    #[tokio::test]
    async fn refresh_projection_populates_data_product_graph() {
        let store = test_store_with_triples();
        let iri_builder = IriBuilder::new(ClusterDomain::default());
        let product_graph_iri = format!("{}/graph", iri_builder.product("myapp").as_str());

        let projector = OxigraphDataProductProjector::new(Arc::new(store));

        let dp_iri = ResourceIri::new("https://picloud.local/data-products/myapp/things-export").unwrap();
        let source_iri = ResourceIri::new(&product_graph_iri).unwrap();

        let construct = format!(
            "CONSTRUCT {{ ?s ?p ?o }} WHERE {{ GRAPH <{product_graph_iri}> {{ ?s ?p ?o }} }}"
        );

        let result = projector
            .refresh_projection(&dp_iri, &construct, &source_iri)
            .await
            .unwrap();

        assert_eq!(result.triple_count, 2);
        assert!(result.duration_ms < 5000);
    }

    #[tokio::test]
    async fn query_data_product_returns_results() {
        let store = test_store_with_triples();
        let iri_builder = IriBuilder::new(ClusterDomain::default());
        let product_graph_iri = format!("{}/graph", iri_builder.product("myapp").as_str());

        let projector = OxigraphDataProductProjector::new(Arc::new(store));

        let dp_iri = ResourceIri::new("https://picloud.local/data-products/myapp/things-export").unwrap();
        let source_iri = ResourceIri::new(&product_graph_iri).unwrap();

        // First populate the data product graph
        let construct = format!(
            "CONSTRUCT {{ ?s ?p ?o }} WHERE {{ GRAPH <{product_graph_iri}> {{ ?s ?p ?o }} }}"
        );
        projector
            .refresh_projection(&dp_iri, &construct, &source_iri)
            .await
            .unwrap();

        // Now query the data product
        let result = projector
            .query_data_product(
                &dp_iri,
                "?s <https://picloud.local/ontology#name> ?name",
            )
            .await
            .unwrap();

        assert_eq!(result.bindings.len(), 1);
        assert_eq!(result.bindings[0]["name"], "Thing One");
    }

    #[tokio::test]
    async fn refresh_replaces_old_triples() {
        let store = Store::new().unwrap();
        let product_graph = "https://picloud.local/products/myapp/graph";

        // Insert initial data
        store
            .insert_named_graph(NamedNodeRef::new(product_graph).unwrap())
            .unwrap();
        let subj = NN::new("https://picloud.local/products/myapp/things/t1").unwrap();
        let pred = NN::new("https://picloud.local/ontology#name").unwrap();
        let obj = Literal::new_simple_literal("Old Name");
        let g = NN::new(product_graph).unwrap();
        store.insert(QR::new(&subj, &pred, &obj, &g)).unwrap();

        let projector = OxigraphDataProductProjector::new(Arc::new(store));
        let dp_iri = ResourceIri::new("https://picloud.local/data-products/myapp/export").unwrap();
        let source_iri = ResourceIri::new(product_graph).unwrap();

        let construct = format!(
            "CONSTRUCT {{ ?s ?p ?o }} WHERE {{ GRAPH <{product_graph}> {{ ?s ?p ?o }} }}"
        );

        // First refresh: 1 triple
        let r1 = projector
            .refresh_projection(&dp_iri, &construct, &source_iri)
            .await
            .unwrap();
        assert_eq!(r1.triple_count, 1);

        // Second refresh: still 1 triple (old ones cleared)
        let r2 = projector
            .refresh_projection(&dp_iri, &construct, &source_iri)
            .await
            .unwrap();
        assert_eq!(r2.triple_count, 1);

        // Verify only 1 triple in the dp graph
        let result = projector
            .query_data_product(&dp_iri, "?s ?p ?o")
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
    }

    #[tokio::test]
    async fn non_construct_query_returns_error() {
        let store = Store::new().unwrap();
        let projector = OxigraphDataProductProjector::new(Arc::new(store));
        let dp_iri = ResourceIri::new("https://picloud.local/data-products/myapp/export").unwrap();
        let source_iri = ResourceIri::new("https://picloud.local/products/myapp/graph").unwrap();

        let result = projector
            .refresh_projection(&dp_iri, "SELECT * WHERE { ?s ?p ?o }", &source_iri)
            .await;

        assert!(result.is_err());
    }
}
