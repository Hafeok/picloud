//! OxigraphProjector — projects platform events into an RDF triplestore.
//!
//! Supports both in-memory mode (for tests) and disk-backed mode (for
//! production). In disk-backed mode, the Oxigraph store is persisted via
//! RocksDB and survives process restarts. A replay cursor tracks the last
//! projected event offset so that on restart the node only projects events
//! it missed rather than replaying the entire log.
//!
//! **Distribution model**: the RDF graph is NOT replicated directly. The
//! Raft-replicated event log is the shared state. Each node independently
//! projects its own local Oxigraph copy from the log. Because projectors
//! are deterministic, all nodes converge to the same graph.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use oxigraph::model::{
    GraphNameRef, Literal, NamedNode, NamedNodeRef, Quad, QuadRef, SubjectRef, Term,
};
use oxigraph::sparql::QueryResults;
use oxigraph::store::Store;
use tracing::{debug, info, warn};

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{QueryResult, StateProjector};

/// Namespace constants for PiCloud RDF vocabulary.
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const PICLOUD_NS: &str = "https://picloud.local/ontology#";

/// Constructs a PiCloud ontology IRI, e.g. `https://picloud.local/ontology#Node`.
fn picloud_term(local: &str) -> NamedNode {
    NamedNode::new(format!("{PICLOUD_NS}{local}")).expect("valid picloud ontology IRI")
}

/// The Oxigraph-backed implementation of `StateProjector`.
///
/// Tracks a replay cursor (`last_projected_offset`) so that on startup
/// the node can call `replay()` with events from the log and only project
/// those it hasn't seen yet. The cursor is atomically incremented after
/// each successful projection.
pub struct OxigraphProjector {
    store: Store,
    iri_builder: IriBuilder,
    /// The number of events this projector has processed. On startup this
    /// is 0 (fresh/in-memory) or restored from a metadata triple in the
    /// store (disk-backed). After projecting event at log index N the
    /// cursor becomes N+1, meaning "give me events starting at N+1".
    last_projected_offset: AtomicUsize,
}

impl OxigraphProjector {
    /// Create a new **in-memory** projector (for tests and single-run usage).
    pub fn new() -> Result<Self> {
        let store = Store::new().map_err(|e| PiCloudError::Internal(e.to_string()))?;
        Ok(Self {
            store,
            iri_builder: IriBuilder::new(ClusterDomain::default()),
            last_projected_offset: AtomicUsize::new(0),
        })
    }

    /// Create a new in-memory projector with a custom cluster domain.
    pub fn with_domain(domain: ClusterDomain) -> Result<Self> {
        let store = Store::new().map_err(|e| PiCloudError::Internal(e.to_string()))?;
        Ok(Self {
            store,
            iri_builder: IriBuilder::new(domain),
            last_projected_offset: AtomicUsize::new(0),
        })
    }

    /// Open a **disk-backed** projector at the given path.
    ///
    /// If the directory already contains an Oxigraph store, it is reopened
    /// and the replay cursor is restored from a metadata triple stored in
    /// the graph. This means a restarting node skips events it already
    /// projected.
    ///
    /// If the directory is empty, a fresh store is created and the cursor
    /// starts at 0 — the node will replay the entire log on first boot.
    ///
    /// When compiled without the `rocksdb` feature, this falls back to an
    /// in-memory store (the path is ignored).
    pub fn open(path: impl AsRef<Path>, domain: ClusterDomain) -> Result<Self> {
        #[cfg(feature = "rocksdb")]
        {
            let store = Store::open(path.as_ref())
                .map_err(|e| PiCloudError::Internal(format!(
                    "Failed to open Oxigraph store at {}: {e}",
                    path.as_ref().display(),
                )))?;

            let cursor = Self::read_cursor_from_store(&store).unwrap_or(0);

            info!(
                path = %path.as_ref().display(),
                restored_cursor = cursor,
                "Opened disk-backed Oxigraph store"
            );

            return Ok(Self {
                store,
                iri_builder: IriBuilder::new(domain),
                last_projected_offset: AtomicUsize::new(cursor),
            });
        }

        #[cfg(not(feature = "rocksdb"))]
        {
            warn!(
                path = %path.as_ref().display(),
                "Disk-backed store requires rocksdb feature — using in-memory store"
            );
            Self::with_domain(domain)
        }
    }

    /// The current replay cursor — the next log offset the projector
    /// expects. Pass this to `event_log.events_since(cursor)` to get
    /// the events the projector hasn't seen.
    pub fn cursor(&self) -> usize {
        self.last_projected_offset.load(Ordering::Relaxed)
    }

    /// Replay a batch of historical events (e.g. on startup catchup).
    ///
    /// Projects each event in order and advances the cursor. This is the
    /// mechanism that gets a new or restarted node up to speed:
    ///
    /// ```text
    /// let missed = event_log.events_since(projector.cursor()).await;
    /// projector.replay(&missed).await?;
    /// // projector is now caught up — subscribe to live events
    /// ```
    pub async fn replay(&self, events: &[EventEnvelope]) -> Result<usize> {
        let mut projected = 0;
        for event in events {
            self.project(event).await?;
            projected += 1;
        }
        if projected > 0 {
            self.persist_cursor()?;
            info!(events_projected = projected, cursor = self.cursor(), "Replay complete");
        }
        Ok(projected)
    }

    /// Returns a reference to the `IriBuilder`.
    pub fn iri_builder(&self) -> &IriBuilder {
        &self.iri_builder
    }

    /// Persist the current cursor value into the store as a metadata triple.
    /// Only meaningful for disk-backed stores (in-memory stores lose it anyway).
    fn persist_cursor(&self) -> Result<()> {
        let meta_subject = "https://picloud.local/meta/projector";
        let meta_predicate = &format!("{PICLOUD_NS}lastProjectedOffset");
        let cursor = self.cursor();

        // Remove old cursor triple
        let s = NamedNode::new(meta_subject)
            .map_err(|e| PiCloudError::Internal(format!("invalid meta IRI: {e}")))?;
        let p = NamedNodeRef::new(meta_predicate)
            .map_err(|e| PiCloudError::Internal(format!("invalid meta predicate: {e}")))?;

        let old_quads: Vec<Quad> = self
            .store
            .quads_for_pattern(
                Some(SubjectRef::from(&s)),
                Some(p),
                None,
                Some(GraphNameRef::DefaultGraph),
            )
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;
        for quad in &old_quads {
            let _ = self.store.remove(quad);
        }

        // Write new cursor
        self.insert_triple(
            meta_subject,
            meta_predicate,
            Literal::new_simple_literal(cursor.to_string()).into(),
        )?;

        // Flush to disk if the store supports it (rocksdb feature)
        #[cfg(feature = "rocksdb")]
        self.store.flush().map_err(|e| PiCloudError::Internal(e.to_string()))?;

        Ok(())
    }

    /// Read the replay cursor from the store's metadata triple.
    #[allow(dead_code)]
    fn read_cursor_from_store(store: &Store) -> Option<usize> {
        let sparql = format!(
            "SELECT ?offset WHERE {{ <https://picloud.local/meta/projector> <{PICLOUD_NS}lastProjectedOffset> ?offset }}"
        );
        if let Ok(QueryResults::Solutions(solutions)) = store.query(&sparql) {
            for solution in solutions.flatten() {
                if let Some(Term::Literal(lit)) = solution.get("offset") {
                    return lit.value().parse::<usize>().ok();
                }
            }
        }
        None
    }

    /// Insert a single triple into the default graph.
    pub fn insert_triple(
        &self,
        subject: &str,
        predicate: &str,
        object: Term,
    ) -> Result<()> {
        let s = NamedNode::new(subject)
            .map_err(|e| PiCloudError::Internal(format!("invalid subject IRI: {e}")))?;
        let p = NamedNode::new(predicate)
            .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;
        self.store
            .insert(QuadRef::new(
                &s,
                &p,
                &object,
                GraphNameRef::DefaultGraph,
            ))
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;
        Ok(())
    }

    /// Insert a triple into a named graph (for product-scoped data).
    fn insert_triple_in_graph(
        &self,
        subject: &str,
        predicate: &str,
        object: Term,
        graph: &str,
    ) -> Result<()> {
        let s = NamedNode::new(subject)
            .map_err(|e| PiCloudError::Internal(format!("invalid subject IRI: {e}")))?;
        let p = NamedNode::new(predicate)
            .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;
        let g = NamedNode::new(graph)
            .map_err(|e| PiCloudError::Internal(format!("invalid graph IRI: {e}")))?;
        // Ensure the named graph exists.
        self.store
            .insert_named_graph(NamedNodeRef::new(graph).unwrap())
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;
        self.store
            .insert(QuadRef::new(&s, &p, &object, &g))
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;
        Ok(())
    }

    /// Remove all triples where `subject` appears as the subject (default graph).
    pub fn remove_triples_about(&self, subject: &str) -> Result<()> {
        let s = NamedNode::new(subject)
            .map_err(|e| PiCloudError::Internal(format!("invalid subject IRI: {e}")))?;
        let quads: Vec<Quad> = self
            .store
            .quads_for_pattern(
                Some(SubjectRef::from(&s)),
                None,
                None,
                Some(GraphNameRef::DefaultGraph),
            )
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;
        for quad in &quads {
            self.store
                .remove(quad)
                .map_err(|e| PiCloudError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    /// Remove all triples about a subject in all graphs (default + named).
    fn remove_triples_about_all_graphs(&self, subject: &str) -> Result<()> {
        let s = NamedNode::new(subject)
            .map_err(|e| PiCloudError::Internal(format!("invalid subject IRI: {e}")))?;
        let quads: Vec<Quad> = self
            .store
            .quads_for_pattern(Some(SubjectRef::from(&s)), None, None, None)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;
        for quad in &quads {
            self.store
                .remove(quad)
                .map_err(|e| PiCloudError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    /// Execute a SPARQL SELECT and convert results to JSON bindings.
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

    // ---- Event projection handlers ----

    fn project_node_joined(&self, event: &EventEnvelope) -> Result<()> {
        let node_id = event.payload["node_id"]
            .as_str()
            .unwrap_or_default();
        let node_name = event.payload["node_name"]
            .as_str()
            .unwrap_or_default();
        let node_iri_str = event.payload["node_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let address = event.payload["address"]
            .as_str()
            .unwrap_or_default();

        self.insert_triple(
            node_iri_str,
            RDF_TYPE,
            picloud_term("Node").into(),
        )?;
        self.insert_triple(
            node_iri_str,
            &format!("{PICLOUD_NS}nodeId"),
            Literal::new_simple_literal(node_id).into(),
        )?;
        self.insert_triple(
            node_iri_str,
            &format!("{PICLOUD_NS}nodeName"),
            Literal::new_simple_literal(node_name).into(),
        )?;
        self.insert_triple(
            node_iri_str,
            &format!("{PICLOUD_NS}address"),
            Literal::new_simple_literal(address).into(),
        )?;
        self.insert_triple(
            node_iri_str,
            &format!("{PICLOUD_NS}status"),
            Literal::new_simple_literal("joined").into(),
        )?;

        debug!(node_iri = node_iri_str, "projected NodeJoined");
        Ok(())
    }

    fn project_node_left(&self, event: &EventEnvelope) -> Result<()> {
        let node_iri_str = event.payload["node_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());

        self.remove_triples_about_all_graphs(node_iri_str)?;
        debug!(node_iri = node_iri_str, "projected NodeLeft");
        Ok(())
    }

    fn project_resource_declared(&self, event: &EventEnvelope) -> Result<()> {
        let resource_iri_str = event.payload["resource_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let resource_type = event.payload["resource_type"]
            .as_str()
            .unwrap_or("Resource");

        // Insert into default graph.
        self.insert_triple(
            resource_iri_str,
            RDF_TYPE,
            picloud_term("Resource").into(),
        )?;
        self.insert_triple(
            resource_iri_str,
            &format!("{PICLOUD_NS}resourceType"),
            Literal::new_simple_literal(resource_type).into(),
        )?;
        self.insert_triple(
            resource_iri_str,
            &format!("{PICLOUD_NS}status"),
            Literal::new_simple_literal("declared").into(),
        )?;

        // If product-scoped, also insert into the product's named graph.
        if let Some(product) = event.payload["product"].as_str() {
            let graph_iri = self.iri_builder.product_graph(product);
            self.insert_triple_in_graph(
                resource_iri_str,
                RDF_TYPE,
                picloud_term("Resource").into(),
                graph_iri.as_str(),
            )?;
            self.insert_triple_in_graph(
                resource_iri_str,
                &format!("{PICLOUD_NS}resourceType"),
                Literal::new_simple_literal(resource_type).into(),
                graph_iri.as_str(),
            )?;
            self.insert_triple_in_graph(
                resource_iri_str,
                &format!("{PICLOUD_NS}status"),
                Literal::new_simple_literal("declared").into(),
                graph_iri.as_str(),
            )?;
        }

        debug!(resource_iri = resource_iri_str, "projected ResourceDeclared");
        Ok(())
    }

    fn project_resource_ready(&self, event: &EventEnvelope) -> Result<()> {
        let resource_iri_str = event.payload["resource_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());

        self.update_status(resource_iri_str, "ready", event.product.as_deref())?;
        debug!(resource_iri = resource_iri_str, "projected ResourceReady");
        Ok(())
    }

    fn project_resource_deleted(&self, event: &EventEnvelope) -> Result<()> {
        let resource_iri_str = event.payload["resource_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());

        // Remove from all graphs (default + product named graph)
        self.remove_triples_about_all_graphs(resource_iri_str)?;
        debug!(resource_iri = resource_iri_str, "projected ResourceDeleted");
        Ok(())
    }

    fn project_identity_created(&self, event: &EventEnvelope) -> Result<()> {
        let identity_iri_str = event.payload["identity_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let identity_type = event.payload["identity_type"]
            .as_str()
            .unwrap_or("Identity");
        let name = event.payload["name"]
            .as_str()
            .unwrap_or_default();

        self.insert_triple(
            identity_iri_str,
            RDF_TYPE,
            picloud_term("Identity").into(),
        )?;
        self.insert_triple(
            identity_iri_str,
            &format!("{PICLOUD_NS}identityType"),
            Literal::new_simple_literal(identity_type).into(),
        )?;
        self.insert_triple(
            identity_iri_str,
            &format!("{PICLOUD_NS}name"),
            Literal::new_simple_literal(name).into(),
        )?;
        self.insert_triple(
            identity_iri_str,
            &format!("{PICLOUD_NS}status"),
            Literal::new_simple_literal("active").into(),
        )?;

        // If product-scoped, also insert into the product's named graph
        if let Some(product) = event.payload["product"].as_str() {
            let graph_iri = self.iri_builder.product_graph(product);
            self.insert_triple_in_graph(
                identity_iri_str,
                RDF_TYPE,
                picloud_term("Identity").into(),
                graph_iri.as_str(),
            )?;
            self.insert_triple_in_graph(
                identity_iri_str,
                &format!("{PICLOUD_NS}name"),
                Literal::new_simple_literal(name).into(),
                graph_iri.as_str(),
            )?;
        }

        debug!(identity_iri = identity_iri_str, "projected IdentityCreated");
        Ok(())
    }

    fn project_product_deployed(&self, event: &EventEnvelope) -> Result<()> {
        let product_iri_str = event.payload["product_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let product_name = event.payload["product_name"]
            .as_str()
            .unwrap_or_default();
        let version = event.payload["version"]
            .as_str()
            .unwrap_or("0.0.0");

        // Insert product as a Resource with type "Product"
        self.insert_triple(
            product_iri_str,
            RDF_TYPE,
            picloud_term("Resource").into(),
        )?;
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}resourceType"),
            Literal::new_simple_literal("Product").into(),
        )?;
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}name"),
            Literal::new_simple_literal(product_name).into(),
        )?;
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}version"),
            Literal::new_simple_literal(version).into(),
        )?;
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}status"),
            Literal::new_simple_literal("deployed").into(),
        )?;

        // Register SPARQL endpoint and event stream IRIs
        let graph_iri = self.iri_builder.product_graph(product_name);
        let events_iri = self.iri_builder.product_events(product_name);
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}sparqlEndpoint"),
            NamedNode::new(graph_iri.as_str())
                .map_err(|e| PiCloudError::Internal(e.to_string()))?
                .into(),
        )?;
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}eventStream"),
            NamedNode::new(events_iri.as_str())
                .map_err(|e| PiCloudError::Internal(e.to_string()))?
                .into(),
        )?;

        debug!(product_iri = product_iri_str, "projected ProductDeployed");
        Ok(())
    }

    fn project_resource_failed(&self, event: &EventEnvelope) -> Result<()> {
        let resource_iri_str = event.payload["resource_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let reason = event.payload["reason"]
            .as_str()
            .unwrap_or("unknown");

        self.update_status(resource_iri_str, "failed", event.product.as_deref())?;

        // Add failure reason triple.
        self.insert_triple(
            resource_iri_str,
            &format!("{PICLOUD_NS}failureReason"),
            Literal::new_simple_literal(reason).into(),
        )?;
        if let Some(product) = &event.product {
            let graph_iri = self.iri_builder.product_graph(product);
            self.insert_triple_in_graph(
                resource_iri_str,
                &format!("{PICLOUD_NS}failureReason"),
                Literal::new_simple_literal(reason).into(),
                graph_iri.as_str(),
            )?;
        }

        debug!(resource_iri = resource_iri_str, "projected ResourceFailed");
        Ok(())
    }

    /// Replace the status literal for a resource in the default graph
    /// (and optionally a product graph).
    fn update_status(
        &self,
        resource_iri: &str,
        new_status: &str,
        product: Option<&str>,
    ) -> Result<()> {
        let s = NamedNode::new(resource_iri)
            .map_err(|e| PiCloudError::Internal(format!("invalid subject IRI: {e}")))?;
        let status_pred = format!("{PICLOUD_NS}status");
        let p = NamedNodeRef::new(&status_pred)
            .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;

        // Remove old status triples in default graph.
        let old_quads: Vec<Quad> = self
            .store
            .quads_for_pattern(
                Some(SubjectRef::from(&s)),
                Some(p),
                None,
                Some(GraphNameRef::DefaultGraph),
            )
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;
        for quad in &old_quads {
            self.store
                .remove(quad)
                .map_err(|e| PiCloudError::Internal(e.to_string()))?;
        }

        // Insert new status.
        self.insert_triple(
            resource_iri,
            &status_pred,
            Literal::new_simple_literal(new_status).into(),
        )?;

        // Update in product named graph if applicable.
        if let Some(product_name) = product {
            let graph_iri = self.iri_builder.product_graph(product_name);
            let g = NamedNode::new(graph_iri.as_str())
                .map_err(|e| PiCloudError::Internal(format!("invalid graph IRI: {e}")))?;
            let old_quads: Vec<Quad> = self
                .store
                .quads_for_pattern(
                    Some(SubjectRef::from(&s)),
                    Some(p),
                    None,
                    Some(GraphNameRef::from(&g)),
                )
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| PiCloudError::Internal(e.to_string()))?;
            for quad in &old_quads {
                self.store
                    .remove(quad)
                    .map_err(|e| PiCloudError::Internal(e.to_string()))?;
            }
            self.insert_triple_in_graph(
                resource_iri,
                &status_pred,
                Literal::new_simple_literal(new_status).into(),
                graph_iri.as_str(),
            )?;
        }

        Ok(())
    }
}

/// Convert an oxigraph `Term` to a JSON value following SPARQL JSON results conventions.
fn term_to_json(term: &Term) -> serde_json::Value {
    match term {
        Term::NamedNode(n) => serde_json::json!({
            "type": "uri",
            "value": n.as_str(),
        }),
        Term::BlankNode(b) => serde_json::json!({
            "type": "bnode",
            "value": b.as_str(),
        }),
        Term::Literal(lit) => {
            let mut obj = serde_json::Map::new();
            obj.insert("type".into(), serde_json::Value::String("literal".into()));
            obj.insert(
                "value".into(),
                serde_json::Value::String(lit.value().to_string()),
            );
            if let Some(lang) = lit.language() {
                obj.insert(
                    "xml:lang".into(),
                    serde_json::Value::String(lang.to_string()),
                );
            } else {
                let dt = lit.datatype();
                if dt.as_str() != "http://www.w3.org/2001/XMLSchema#string" {
                    obj.insert(
                        "datatype".into(),
                        serde_json::Value::String(dt.as_str().to_string()),
                    );
                }
            }
            serde_json::Value::Object(obj)
        }
        #[allow(unreachable_patterns)]
        _ => serde_json::Value::String(term.to_string()),
    }
}

#[async_trait]
impl StateProjector for OxigraphProjector {
    async fn project(&self, event: &EventEnvelope) -> Result<()> {
        let result = match event.event_type.as_str() {
            "NodeJoined" => self.project_node_joined(event),
            "NodeLeft" => self.project_node_left(event),
            "ResourceDeclared" => self.project_resource_declared(event),
            "ResourceReady" => self.project_resource_ready(event),
            "ResourceFailed" => self.project_resource_failed(event),
            "ResourceDeleted" => self.project_resource_deleted(event),
            "IdentityCreated" => self.project_identity_created(event),
            "ProductDeployed" => self.project_product_deployed(event),
            other => {
                debug!(event_type = other, "unhandled event type — skipping projection");
                Ok(())
            }
        };

        if result.is_ok() {
            self.last_projected_offset.fetch_add(1, Ordering::Relaxed);
        } else {
            warn!(
                event_id = %event.id,
                event_type = %event.event_type,
                "Projection failed — cursor NOT advanced"
            );
        }

        result
    }

    async fn query(&self, sparql: &str) -> Result<QueryResult> {
        self.execute_query(sparql)
    }

    async fn query_product(
        &self,
        product_iri: &ResourceIri,
        sparql: &str,
    ) -> Result<QueryResult> {
        // For product queries we wrap the user's pattern in a GRAPH clause
        // restricting results to the product's named graph.
        let product_graph = format!("{}/graph", product_iri.as_str());
        let wrapped = format!(
            "SELECT * WHERE {{ GRAPH <{product_graph}> {{ {sparql} }} }}"
        );
        self.execute_query(&wrapped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picloud_domain::iri::{ClusterDomain, IriBuilder};
    use uuid::Uuid;

    fn make_iri_builder() -> IriBuilder {
        IriBuilder::new(ClusterDomain::default())
    }

    fn make_node_joined_event(iri_builder: &IriBuilder) -> EventEnvelope {
        let node_iri = iri_builder.node("pi-node-01");
        EventEnvelope::new(
            iri_builder.event_schema("NodeJoined", 1),
            "NodeJoined",
            node_iri.clone(),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "node_id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                "node_name": "pi-node-01",
                "node_iri": node_iri.as_str(),
                "address": "192.168.1.10:9000",
            }),
        )
    }

    #[tokio::test]
    async fn test_project_node_joined_and_query() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();
        let event = make_node_joined_event(&iri_builder);

        projector.project(&event).await.unwrap();

        let result = projector
            .query("SELECT ?node ?addr WHERE { ?node <https://picloud.local/ontology#address> ?addr }")
            .await
            .unwrap();

        assert_eq!(result.bindings.len(), 1);
        let row = &result.bindings[0];
        assert_eq!(
            row["addr"]["value"].as_str().unwrap(),
            "192.168.1.10:9000"
        );
        assert_eq!(
            row["node"]["value"].as_str().unwrap(),
            "https://picloud.local/nodes/pi-node-01"
        );
    }

    #[tokio::test]
    async fn test_project_node_left_removes_triples() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();

        let join_event = make_node_joined_event(&iri_builder);
        projector.project(&join_event).await.unwrap();

        let leave_event = EventEnvelope::new(
            iri_builder.event_schema("NodeLeft", 1),
            "NodeLeft",
            iri_builder.node("pi-node-01"),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "node_id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                "node_iri": "https://picloud.local/nodes/pi-node-01",
                "reason": "graceful shutdown",
            }),
        );
        projector.project(&leave_event).await.unwrap();

        let result = projector
            .query("SELECT ?node WHERE { ?node <https://picloud.local/ontology#address> ?addr }")
            .await
            .unwrap();
        assert!(result.bindings.is_empty());
    }

    #[tokio::test]
    async fn test_project_resource_declared_and_ready() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();
        let resource_iri = iri_builder.resource("photo-app", "containers", "api-server");

        let declared_event = EventEnvelope::new(
            iri_builder.event_schema("ResourceDeclared", 1),
            "ResourceDeclared",
            resource_iri.clone(),
            Some("photo-app".to_string()),
            Uuid::new_v4(),
            serde_json::json!({
                "resource_iri": resource_iri.as_str(),
                "resource_type": "Container",
                "product": "photo-app",
            }),
        );
        projector.project(&declared_event).await.unwrap();

        // Verify declared status.
        let result = projector
            .query(&format!(
                "SELECT ?status WHERE {{ <{}> <{}status> ?status }}",
                resource_iri.as_str(),
                PICLOUD_NS,
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(
            result.bindings[0]["status"]["value"].as_str().unwrap(),
            "declared"
        );

        // Mark ready.
        let ready_event = EventEnvelope::new(
            iri_builder.event_schema("ResourceReady", 1),
            "ResourceReady",
            resource_iri.clone(),
            Some("photo-app".to_string()),
            Uuid::new_v4(),
            serde_json::json!({
                "resource_iri": resource_iri.as_str(),
            }),
        );
        projector.project(&ready_event).await.unwrap();

        // Verify updated status.
        let result = projector
            .query(&format!(
                "SELECT ?status WHERE {{ <{}> <{}status> ?status }}",
                resource_iri.as_str(),
                PICLOUD_NS,
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(
            result.bindings[0]["status"]["value"].as_str().unwrap(),
            "ready"
        );
    }

    #[tokio::test]
    async fn test_project_resource_failed_with_reason() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();
        let resource_iri = iri_builder.resource("photo-app", "volumes", "data-vol");

        let declared_event = EventEnvelope::new(
            iri_builder.event_schema("ResourceDeclared", 1),
            "ResourceDeclared",
            resource_iri.clone(),
            Some("photo-app".to_string()),
            Uuid::new_v4(),
            serde_json::json!({
                "resource_iri": resource_iri.as_str(),
                "resource_type": "Volume",
                "product": "photo-app",
            }),
        );
        projector.project(&declared_event).await.unwrap();

        let failed_event = EventEnvelope::new(
            iri_builder.event_schema("ResourceFailed", 1),
            "ResourceFailed",
            resource_iri.clone(),
            Some("photo-app".to_string()),
            Uuid::new_v4(),
            serde_json::json!({
                "resource_iri": resource_iri.as_str(),
                "reason": "disk full",
            }),
        );
        projector.project(&failed_event).await.unwrap();

        let result = projector
            .query(&format!(
                "SELECT ?status ?reason WHERE {{ <{0}> <{1}status> ?status . <{0}> <{1}failureReason> ?reason }}",
                resource_iri.as_str(),
                PICLOUD_NS,
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(
            result.bindings[0]["status"]["value"].as_str().unwrap(),
            "failed"
        );
        assert_eq!(
            result.bindings[0]["reason"]["value"].as_str().unwrap(),
            "disk full"
        );
    }

    #[tokio::test]
    async fn test_query_product_named_graph() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();
        let resource_iri = iri_builder.resource("photo-app", "containers", "api-server");
        let product_iri = iri_builder.product("photo-app");

        let declared_event = EventEnvelope::new(
            iri_builder.event_schema("ResourceDeclared", 1),
            "ResourceDeclared",
            resource_iri.clone(),
            Some("photo-app".to_string()),
            Uuid::new_v4(),
            serde_json::json!({
                "resource_iri": resource_iri.as_str(),
                "resource_type": "Container",
                "product": "photo-app",
            }),
        );
        projector.project(&declared_event).await.unwrap();

        let result = projector
            .query_product(
                &product_iri,
                &format!("?res <{}resourceType> ?rtype", PICLOUD_NS),
            )
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(
            result.bindings[0]["rtype"]["value"].as_str().unwrap(),
            "Container"
        );
    }

    #[tokio::test]
    async fn test_cursor_advances_on_projection() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();

        assert_eq!(projector.cursor(), 0);

        let event = make_node_joined_event(&iri_builder);
        projector.project(&event).await.unwrap();
        assert_eq!(projector.cursor(), 1);

        // Unknown events also advance the cursor (they succeed with a skip)
        let unknown = EventEnvelope::new(
            iri_builder.event_schema("Unknown", 1),
            "Unknown",
            ResourceIri::new("https://picloud.local/test").unwrap(),
            None,
            Uuid::new_v4(),
            serde_json::json!({}),
        );
        projector.project(&unknown).await.unwrap();
        assert_eq!(projector.cursor(), 2);
    }

    #[tokio::test]
    async fn test_replay_batch() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();

        let events = vec![
            make_node_joined_event(&iri_builder),
            EventEnvelope::new(
                iri_builder.event_schema("ResourceDeclared", 1),
                "ResourceDeclared",
                iri_builder.resource("photo-app", "containers", "api"),
                Some("photo-app".to_string()),
                Uuid::new_v4(),
                serde_json::json!({
                    "resource_iri": iri_builder.resource("photo-app", "containers", "api").as_str(),
                    "resource_type": "Container",
                    "product": "photo-app",
                }),
            ),
        ];

        let count = projector.replay(&events).await.unwrap();
        assert_eq!(count, 2);
        assert_eq!(projector.cursor(), 2);

        // Verify data was actually projected
        let result = projector
            .query("SELECT ?s WHERE { ?s a <https://picloud.local/ontology#Node> }")
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
    }

    #[tokio::test]
    async fn test_unhandled_event_skips_gracefully() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();

        let event = EventEnvelope::new(
            iri_builder.event_schema("SomeUnknownEvent", 1),
            "SomeUnknownEvent",
            ResourceIri::new("https://picloud.local/some/resource").unwrap(),
            None,
            Uuid::new_v4(),
            serde_json::json!({}),
        );

        projector.project(&event).await.unwrap();

        let result = projector
            .query("SELECT ?s ?p ?o WHERE { ?s ?p ?o }")
            .await
            .unwrap();
        assert!(result.bindings.is_empty());
    }
}
