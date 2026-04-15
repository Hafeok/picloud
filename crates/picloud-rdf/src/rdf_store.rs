//! Per-product RDF store manager (ADR-019, FT-051).
//!
//! Each Product that declares an `rdf-store` resource gets a dedicated
//! Oxigraph instance.  The instance supports full SPARQL 1.1 Query and
//! Update and is isolated from other products' stores.
//!
//! In-memory mode (for tests) creates stores on the heap.  Disk-backed
//! mode (when `rocksdb` feature is enabled) persists each store in a
//! product-scoped subdirectory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use async_trait::async_trait;
use oxigraph::sparql::QueryResults;
use oxigraph::store::Store;
use tracing::{debug, info};

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::traits::{QueryResult, RdfStoreManager};

/// Manages per-product Oxigraph instances.
///
/// Thread-safe: inner map is behind an `RwLock` so multiple readers can
/// query concurrently while creation/deletion takes a write lock.
pub struct OxigraphRdfStoreManager {
    stores: RwLock<HashMap<String, Store>>,
    /// Base directory for disk-backed stores. `None` → in-memory only.
    base_path: Option<PathBuf>,
}

impl OxigraphRdfStoreManager {
    /// Create a manager that keeps all stores in memory (for tests).
    pub fn new() -> Self {
        Self {
            stores: RwLock::new(HashMap::new()),
            base_path: None,
        }
    }

    /// Create a manager that persists stores under `base_path/{product}/`.
    #[allow(dead_code)]
    pub fn with_base_path(base_path: impl AsRef<Path>) -> Self {
        Self {
            stores: RwLock::new(HashMap::new()),
            base_path: Some(base_path.as_ref().to_path_buf()),
        }
    }

    /// Open or create the underlying Oxigraph `Store` for a product.
    fn open_store(&self, product_name: &str) -> Result<Store> {
        if let Some(ref base) = self.base_path {
            let dir = base.join(product_name);
            std::fs::create_dir_all(&dir).map_err(|e| {
                PiCloudError::Internal(format!(
                    "failed to create RDF store directory {}: {e}",
                    dir.display()
                ))
            })?;
            #[cfg(feature = "rocksdb")]
            {
                return Store::open(&dir).map_err(|e| {
                    PiCloudError::Internal(format!(
                        "failed to open RDF store at {}: {e}",
                        dir.display()
                    ))
                });
            }
            #[cfg(not(feature = "rocksdb"))]
            {
                tracing::warn!(
                    "rocksdb feature not enabled — using in-memory store for {}",
                    product_name
                );
                Store::new().map_err(|e| PiCloudError::Internal(e.to_string()))
            }
        } else {
            Store::new().map_err(|e| PiCloudError::Internal(e.to_string()))
        }
    }

    /// Execute a SPARQL query against an existing store, returning
    /// `QueryResult` in the same format used by the platform projector.
    fn exec_query(store: &Store, sparql: &str) -> Result<QueryResult> {
        let results = store
            .query(sparql)
            .map_err(|e| PiCloudError::Internal(format!("SPARQL query error: {e}")))?;

        match results {
            QueryResults::Solutions(solutions) => {
                let vars: Vec<String> = solutions
                    .variables()
                    .iter()
                    .map(|v| v.as_str().to_string())
                    .collect();
                let mut bindings = Vec::new();
                for solution in solutions {
                    let solution = solution
                        .map_err(|e| PiCloudError::Internal(format!("solution error: {e}")))?;
                    let mut row = serde_json::Map::new();
                    for var_name in &vars {
                        if let Some(term) =
                            solution.get(var_name.as_str())
                        {
                            row.insert(
                                var_name.clone(),
                                term_to_json(term),
                            );
                        }
                    }
                    bindings.push(serde_json::Value::Object(row));
                }
                Ok(QueryResult { bindings })
            }
            QueryResults::Boolean(b) => Ok(QueryResult {
                bindings: vec![serde_json::json!({"result": b})],
            }),
            QueryResults::Graph(triples) => {
                let mut bindings = Vec::new();
                for triple in triples {
                    let triple = triple
                        .map_err(|e| PiCloudError::Internal(format!("graph error: {e}")))?;
                    bindings.push(serde_json::json!({
                        "subject": term_to_json(&triple.subject.into()),
                        "predicate": term_to_json(&triple.predicate.into()),
                        "object": term_to_json(&triple.object),
                    }));
                }
                Ok(QueryResult { bindings })
            }
        }
    }
}

/// Convert an Oxigraph `Term` to a SPARQL-JSON-style value.
fn term_to_json(term: &oxigraph::model::Term) -> serde_json::Value {
    match term {
        oxigraph::model::Term::NamedNode(n) => serde_json::json!({
            "type": "uri",
            "value": n.as_str(),
        }),
        oxigraph::model::Term::BlankNode(b) => serde_json::json!({
            "type": "bnode",
            "value": b.as_str(),
        }),
        oxigraph::model::Term::Literal(lit) => {
            let mut v = serde_json::json!({
                "type": "literal",
                "value": lit.value(),
            });
            if let Some(lang) = lit.language() {
                v["xml:lang"] = serde_json::Value::String(lang.to_string());
            }
            if lit.datatype().as_str() != "http://www.w3.org/2001/XMLSchema#string"
                && lit.language().is_none()
            {
                v["datatype"] = serde_json::Value::String(lit.datatype().as_str().to_string());
            }
            v
        }
        #[allow(unreachable_patterns)]
        _ => serde_json::json!(term.to_string()),
    }
}

#[async_trait]
impl RdfStoreManager for OxigraphRdfStoreManager {
    async fn create_store(&self, product_name: &str) -> Result<()> {
        let store = self.open_store(product_name)?;
        {
            let mut map = self.stores.write().map_err(|e| {
                PiCloudError::Internal(format!("RDF store lock poisoned: {e}"))
            })?;
            map.insert(product_name.to_string(), store);
        }
        info!(product = product_name, "created per-product RDF store");
        Ok(())
    }

    async fn query_store(&self, product_name: &str, sparql: &str) -> Result<QueryResult> {
        let map = self.stores.read().map_err(|e| {
            PiCloudError::Internal(format!("RDF store lock poisoned: {e}"))
        })?;
        let store = map.get(product_name).ok_or_else(|| {
            PiCloudError::Internal(format!(
                "no RDF store for product '{product_name}'"
            ))
        })?;
        let result = Self::exec_query(store, sparql)?;
        debug!(product = product_name, bindings = result.bindings.len(), "SPARQL query executed");
        Ok(result)
    }

    async fn update_store(&self, product_name: &str, sparql_update: &str) -> Result<()> {
        let map = self.stores.read().map_err(|e| {
            PiCloudError::Internal(format!("RDF store lock poisoned: {e}"))
        })?;
        let store = map.get(product_name).ok_or_else(|| {
            PiCloudError::Internal(format!(
                "no RDF store for product '{product_name}'"
            ))
        })?;
        store
            .update(sparql_update)
            .map_err(|e| PiCloudError::Internal(format!("SPARQL update error: {e}")))?;
        debug!(product = product_name, "SPARQL update executed");
        Ok(())
    }

    async fn has_store(&self, product_name: &str) -> Result<bool> {
        let map = self.stores.read().map_err(|e| {
            PiCloudError::Internal(format!("RDF store lock poisoned: {e}"))
        })?;
        Ok(map.contains_key(product_name))
    }

    async fn drop_store(&self, product_name: &str) -> Result<()> {
        let mut map = self.stores.write().map_err(|e| {
            PiCloudError::Internal(format!("RDF store lock poisoned: {e}"))
        })?;
        map.remove(product_name);
        info!(product = product_name, "dropped per-product RDF store");
        Ok(())
    }
}
