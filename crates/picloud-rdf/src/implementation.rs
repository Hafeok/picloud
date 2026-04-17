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
    GraphName, GraphNameRef, Literal, NamedNode, NamedNodeRef, Quad, QuadRef, Subject, SubjectRef,
    Term,
};
use oxigraph::sparql::QueryResults;
use oxigraph::store::Store;
use tracing::{debug, info, warn};
use uuid::Uuid;

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{QueryResult, ReplayEngine, ReplayRequest, ReplayResult, StateProjector};

/// Namespace constants for PiCloud RDF vocabulary.
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDFS_SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const PICLOUD_NS: &str = "https://picloud.local/ontology#";
const OWL_TRANSITIVE_PROPERTY: &str = "http://www.w3.org/2002/07/owl#TransitiveProperty";

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

    /// Returns a reference to the underlying Oxigraph store.
    ///
    /// Used by the `DataProductProjector` implementation to run CONSTRUCT
    /// queries and manage data product named graphs.
    pub fn store(&self) -> &Store {
        &self.store
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
    pub fn insert_triple_in_graph(
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
    /// Remove all triples matching a subject + predicate (any object, any graph).
    fn remove_triple(&self, subject: &str, predicate: &str) -> Result<()> {
        let s = NamedNode::new(subject)
            .map_err(|e| PiCloudError::Internal(format!("invalid subject IRI: {e}")))?;
        let p = NamedNode::new(predicate)
            .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;
        let quads: Vec<Quad> = self
            .store
            .quads_for_pattern(
                Some(SubjectRef::from(&s)),
                Some(NamedNodeRef::from(&p)),
                None,
                None,
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
    pub fn execute_query(&self, sparql: &str) -> Result<QueryResult> {
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
            QueryResults::Graph(graph) => {
                let mut bindings = Vec::new();
                for triple_result in graph {
                    let triple = triple_result
                        .map_err(|e| PiCloudError::Internal(format!("graph triple error: {e}")))?;
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

    /// Execute a SPARQL CONSTRUCT/DESCRIBE and serialize the result as Turtle.
    ///
    /// Returns the Turtle string, or an error if the query is not a graph query.
    pub fn execute_query_to_turtle(&self, sparql: &str) -> Result<String> {
        let results = self
            .store
            .query(sparql)
            .map_err(|e| PiCloudError::Internal(format!("SPARQL query failed: {e}")))?;
        match results {
            QueryResults::Graph(graph) => {
                let mut serializer = oxigraph::io::RdfSerializer::from_format(
                    oxigraph::io::RdfFormat::Turtle,
                )
                .for_writer(Vec::new());
                for triple_result in graph {
                    let triple = triple_result
                        .map_err(|e| PiCloudError::Internal(format!("graph triple error: {e}")))?;
                    serializer
                        .serialize_triple(&triple)
                        .map_err(|e| PiCloudError::Internal(format!("turtle serialize error: {e}")))?;
                }
                let output = serializer
                    .finish()
                    .map_err(|e| PiCloudError::Internal(format!("turtle finish error: {e}")))?;
                String::from_utf8(output)
                    .map_err(|e| PiCloudError::Internal(format!("turtle utf8 error: {e}")))
            }
            QueryResults::Solutions(_) | QueryResults::Boolean(_) => {
                // For SELECT/ASK queries, fall back to JSON representation
                let result = self.execute_query(sparql)?;
                serde_json::to_string_pretty(&result.bindings)
                    .map_err(|e| PiCloudError::Internal(format!("json serialize error: {e}")))
            }
        }
    }

    // ---- Governance invariant checks (ADR-056, FT-065) ----

    /// Count the number of data products that belong to a given data domain.
    ///
    /// A data product is a member of a domain when its `pc:belongsToDomain`
    /// triple points to the domain's cluster-scoped IRI.
    pub fn count_data_domain_members(&self, domain_name: &str) -> Result<usize> {
        let cluster_root = self.iri_builder.cluster_root();
        let domain_iri = format!(
            "{}/data-domains/{domain_name}",
            cluster_root.as_str().trim_end_matches('/')
        );
        let query = format!(
            "SELECT (COUNT(?dp) AS ?count) WHERE {{ \
             ?dp <{PICLOUD_NS}belongsToDomain> <{domain_iri}> . \
             }}"
        );
        let result = self.execute_query(&query)?;
        let count = result
            .bindings
            .first()
            .and_then(|b| b.get("count"))
            .and_then(|c| c.get("value"))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        Ok(count)
    }

    /// Return `true` if a data domain with this name already exists.
    ///
    /// Data domain names are cluster-unique — a second declaration with the
    /// same name must be rejected as a duplicate (ADR-056, FT-065, TC-208).
    pub fn data_domain_exists(&self, domain_name: &str) -> Result<bool> {
        let cluster_root = self.iri_builder.cluster_root();
        let domain_iri = format!(
            "{}/data-domains/{domain_name}",
            cluster_root.as_str().trim_end_matches('/')
        );
        let query = format!(
            "ASK {{ <{domain_iri}> <{RDF_TYPE}> <{PICLOUD_NS}DataDomain> }}"
        );
        let result = self.execute_query(&query)?;
        Ok(result
            .bindings
            .first()
            .and_then(|b| b.get("result"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false))
    }

    /// Validate that a data domain can be deleted.
    ///
    /// Per ADR-056, a data domain cannot be deleted while any data product is
    /// assigned to it. Returns `DataDomainDeletionBlocked` with the member
    /// count if any data products still reference the domain.
    pub fn validate_data_domain_deletion(&self, domain_name: &str) -> Result<()> {
        let members = self.count_data_domain_members(domain_name)?;
        if members > 0 {
            return Err(PiCloudError::DataDomainDeletionBlocked {
                domain: domain_name.to_string(),
                members,
            });
        }
        Ok(())
    }

    /// Validate that a data domain declaration is unique (cluster-scoped).
    ///
    /// Returns an error if a data domain with this name already exists.
    pub fn validate_data_domain_unique(&self, domain_name: &str) -> Result<()> {
        if self.data_domain_exists(domain_name)? {
            return Err(PiCloudError::ResourceAlreadyExists {
                iri: format!(
                    "{}/data-domains/{domain_name}",
                    self.iri_builder.cluster_root().as_str().trim_end_matches('/')
                ),
            });
        }
        Ok(())
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

    fn project_leader_elected(&self, event: &EventEnvelope) -> Result<()> {
        let node_iri_str = event.payload["node_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());

        // Remove old leader designation from all nodes
        let sparql = format!(
            "SELECT ?node WHERE {{ ?node <{PICLOUD_NS}hasRole> <{PICLOUD_NS}Leader> }}"
        );
        if let Ok(result) = self.execute_query(&sparql) {
            for row in &result.bindings {
                if let Some(node) = row.get("node").and_then(|v| v.get("value")).and_then(|v| v.as_str()) {
                    if let (Ok(subj), Ok(pred), Ok(obj)) = (
                        NamedNode::new(node),
                        NamedNode::new(format!("{PICLOUD_NS}hasRole")),
                        NamedNode::new(format!("{PICLOUD_NS}Leader")),
                    ) {
                        let _ = self.store.remove(&Quad::new(subj, pred, obj, GraphName::DefaultGraph));
                    }
                }
            }
        }

        // Set the new leader
        self.insert_triple(
            node_iri_str,
            &format!("{PICLOUD_NS}hasRole"),
            picloud_term("Leader").into(),
        )?;

        debug!(leader = node_iri_str, "projected LeaderElected");
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

    /// Update the node's drain status in the RDF graph (upsert pattern).
    fn upsert_node_drain_status(&self, node_iri_str: &str, status: &str) -> Result<()> {
        // Remove old drainStatus triple
        let s = NamedNode::new(node_iri_str)
            .map_err(|e| PiCloudError::Internal(format!("invalid node IRI: {e}")))?;
        let pred = format!("{PICLOUD_NS}drainStatus");
        let p = NamedNodeRef::new(&pred)
            .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;
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

        // Insert new status
        self.insert_triple(
            node_iri_str,
            &format!("{PICLOUD_NS}drainStatus"),
            Literal::new_simple_literal(status).into(),
        )?;
        Ok(())
    }

    fn project_node_cordoned(&self, event: &EventEnvelope) -> Result<()> {
        let node_iri_str = event.payload["node_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        self.upsert_node_drain_status(node_iri_str, "cordoned")?;
        debug!(node_iri = node_iri_str, "projected NodeCordoned");
        Ok(())
    }

    fn project_node_uncordoned(&self, event: &EventEnvelope) -> Result<()> {
        let node_iri_str = event.payload["node_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        self.upsert_node_drain_status(node_iri_str, "active")?;
        debug!(node_iri = node_iri_str, "projected NodeUncordoned");
        Ok(())
    }

    fn project_node_drain_started(&self, event: &EventEnvelope) -> Result<()> {
        let node_iri_str = event.payload["node_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        self.upsert_node_drain_status(node_iri_str, "draining")?;

        // Record workload count being drained
        if let Some(count) = event.payload["workload_count"].as_u64() {
            self.insert_triple(
                node_iri_str,
                &format!("{PICLOUD_NS}drainingWorkloadCount"),
                Literal::new_simple_literal(&count.to_string()).into(),
            )?;
        }

        debug!(node_iri = node_iri_str, "projected NodeDrainStarted");
        Ok(())
    }

    fn project_node_drain_completed(&self, event: &EventEnvelope) -> Result<()> {
        let node_iri_str = event.payload["node_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        self.upsert_node_drain_status(node_iri_str, "drained")?;
        debug!(node_iri = node_iri_str, "projected NodeDrainCompleted");
        Ok(())
    }

    fn project_workload_migrated(&self, event: &EventEnvelope) -> Result<()> {
        let workload_iri_str = event.payload["workload_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let to_node_iri = event.payload["to_node_iri"]
            .as_str()
            .unwrap_or_default();

        // Update the workload's scheduledOn triple
        let s = NamedNode::new(workload_iri_str)
            .map_err(|e| PiCloudError::Internal(format!("invalid workload IRI: {e}")))?;
        let pred = format!("{PICLOUD_NS}scheduledOn");
        let p = NamedNodeRef::new(&pred)
            .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;
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

        if !to_node_iri.is_empty() {
            self.insert_triple(
                workload_iri_str,
                &format!("{PICLOUD_NS}scheduledOn"),
                NamedNode::new(to_node_iri)
                    .map_err(|e| PiCloudError::Internal(format!("invalid node IRI: {e}")))?
                    .into(),
            )?;
        }

        debug!(workload_iri = workload_iri_str, to_node = to_node_iri, "projected WorkloadMigrated");
        Ok(())
    }

    fn project_self_monitoring(&self, event: &EventEnvelope) -> Result<()> {
        let node_iri_str = event.payload["node_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let overall_status = event.payload["overall_status"]
            .as_str()
            .unwrap_or("unknown");

        let s = NamedNode::new(node_iri_str)
            .map_err(|e| PiCloudError::Internal(format!("invalid node IRI: {e}")))?;

        // --- Upsert selfMonitoringStatus ---
        {
            let pred = format!("{PICLOUD_NS}selfMonitoringStatus");
            let p = NamedNodeRef::new(&pred)
                .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;
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
        }

        self.insert_triple(
            node_iri_str,
            &format!("{PICLOUD_NS}selfMonitoringStatus"),
            Literal::new_simple_literal(overall_status).into(),
        )?;

        // --- Upsert selfMonitoringCheckedAt timestamp ---
        {
            let pred = format!("{PICLOUD_NS}selfMonitoringCheckedAt");
            let p = NamedNodeRef::new(&pred)
                .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;
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
        }

        self.insert_triple(
            node_iri_str,
            &format!("{PICLOUD_NS}selfMonitoringCheckedAt"),
            Literal::new_typed_literal(
                event.timestamp.to_rfc3339(),
                NamedNode::new("http://www.w3.org/2001/XMLSchema#dateTime")
                    .expect("valid XSD IRI"),
            )
            .into(),
        )?;

        // --- Remove old hasHealthCheck links and check sub-resources ---
        {
            let pred = format!("{PICLOUD_NS}hasHealthCheck");
            let p = NamedNodeRef::new(&pred)
                .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;
            let old_links: Vec<Quad> = self
                .store
                .quads_for_pattern(
                    Some(SubjectRef::from(&s)),
                    Some(p),
                    None,
                    Some(GraphNameRef::DefaultGraph),
                )
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| PiCloudError::Internal(e.to_string()))?;
            for quad in &old_links {
                // Remove all triples about the old check sub-resource
                if let Term::NamedNode(check_node) = &quad.object {
                    self.remove_triples_about(check_node.as_str())?;
                }
                self.store
                    .remove(quad)
                    .map_err(|e| PiCloudError::Internal(e.to_string()))?;
            }
        }

        // --- Project individual health checks as sub-resources ---
        if let Some(checks) = event.payload["checks"].as_array() {
            for check in checks {
                let check_name = check["check_name"].as_str().unwrap_or("unknown");
                let check_status = check["status"].as_str().unwrap_or("unknown");
                let check_message = check["message"].as_str().unwrap_or("");

                // Build a deterministic IRI for this check: {node_iri}/checks/{check_name}
                let check_iri = format!("{node_iri_str}/checks/{check_name}");

                // Link node -> check
                self.insert_triple(
                    node_iri_str,
                    &format!("{PICLOUD_NS}hasHealthCheck"),
                    NamedNode::new(&check_iri)
                        .map_err(|e| PiCloudError::Internal(format!("invalid check IRI: {e}")))?
                        .into(),
                )?;

                // Type the check
                self.insert_triple(
                    &check_iri,
                    RDF_TYPE,
                    picloud_term("HealthCheck").into(),
                )?;

                // Check name
                self.insert_triple(
                    &check_iri,
                    &format!("{PICLOUD_NS}checkName"),
                    Literal::new_simple_literal(check_name).into(),
                )?;

                // Check status
                self.insert_triple(
                    &check_iri,
                    &format!("{PICLOUD_NS}checkStatus"),
                    Literal::new_simple_literal(check_status).into(),
                )?;

                // Check message
                self.insert_triple(
                    &check_iri,
                    &format!("{PICLOUD_NS}checkMessage"),
                    Literal::new_simple_literal(check_message).into(),
                )?;
            }
        }

        debug!(node_iri = node_iri_str, status = overall_status, "projected SelfMonitoringCheckCompleted");
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
            picloud_term("Declared").into(),
        )?;

        // If product-scoped, link to product in default graph and also insert into named graph.
        if let Some(product) = event.payload["product"].as_str() {
            // Insert product link in default graph so ProductDeleted can find children
            self.insert_triple(
                resource_iri_str,
                &format!("{PICLOUD_NS}product"),
                Literal::new_simple_literal(product).into(),
            )?;

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
                picloud_term("Declared").into(),
                graph_iri.as_str(),
            )?;
        }

        // Project group-specific triples (ADR-037): type and hasRole
        if resource_type == "Group" || resource_type == "group" {
            self.insert_triple(
                resource_iri_str,
                RDF_TYPE,
                picloud_term("Group").into(),
            )?;
            if let Some(roles) = event.payload["roles"].as_array() {
                for role in roles {
                    if let Some(role_name) = role.as_str() {
                        let role_iri = self.iri_builder.resource("platform", "roles", role_name);
                        self.insert_triple(
                            resource_iri_str,
                            &format!("{PICLOUD_NS}hasRole"),
                            NamedNode::new(role_iri.as_str())
                                .map_err(|e| PiCloudError::Internal(format!("invalid role IRI: {e}")))?
                                .into(),
                        )?;
                    }
                }
            }
            if let Some(desc) = event.payload["description"].as_str() {
                if !desc.is_empty() {
                    self.insert_triple(
                        resource_iri_str,
                        &format!("{PICLOUD_NS}description"),
                        Literal::new_simple_literal(desc).into(),
                    )?;
                }
            }
        }

        // Project volume-specific triples (ADR-012, ADR-024, ADR-013)
        if resource_type == "Volume" || resource_type == "volume" {
            self.insert_triple(
                resource_iri_str,
                RDF_TYPE,
                picloud_term("Volume").into(),
            )?;
            if let Some(size_gb) = event.payload.get("size_gb") {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}sizeGb"),
                    Literal::new_simple_literal(&size_gb.to_string()).into(),
                )?;
            }
            // Assign a block device path based on the volume name
            let vol_name = event.payload["name"].as_str().unwrap_or("unknown");
            let block_device = format!("/dev/picloud/{}", vol_name);
            self.insert_triple(
                resource_iri_str,
                &format!("{PICLOUD_NS}blockDevice"),
                Literal::new_simple_literal(&block_device).into(),
            )?;

            // Storage intent: durability tier (ADR-024)
            // The payload may have "FullReplication" (Debug format) or "full-replication" (serde)
            let durability_raw = event.payload["durability"].as_str().unwrap_or("FullReplication");
            let durability_normalized = match durability_raw {
                "FullReplication" | "full-replication" | "full_replication" => "full-replication",
                "Quorum" | "quorum" => "quorum",
                "Local" | "local" => "local",
                "None" | "none" => "none",
                other => other,
            };
            self.insert_triple(
                resource_iri_str,
                &format!("{PICLOUD_NS}durability"),
                Literal::new_simple_literal(durability_normalized).into(),
            )?;

            // Performance tier (ADR-024)
            let performance_raw = event.payload["performance"].as_str().unwrap_or("Standard");
            let performance_normalized = match performance_raw {
                "Standard" | "standard" => "standard",
                "Fast" | "fast" => "fast",
                "Archive" | "archive" => "archive",
                other => other,
            };
            self.insert_triple(
                resource_iri_str,
                &format!("{PICLOUD_NS}performance"),
                Literal::new_simple_literal(performance_normalized).into(),
            )?;

            // Replication factor and count (ADR-013)
            // For full-replication, factor = node count. We read it from the payload
            // or default to cluster size. The provisioner will update these when it
            // knows actual node count, but we set sensible defaults here.
            let replication_factor = event.payload["replication_factor"]
                .as_u64()
                .unwrap_or_else(|| {
                    // Default: for full-replication use node_count from payload,
                    // otherwise 1
                    if durability_normalized == "full-replication" {
                        event.payload["node_count"].as_u64().unwrap_or(1)
                    } else {
                        1
                    }
                });
            self.insert_triple(
                resource_iri_str,
                &format!("{PICLOUD_NS}replicationFactor"),
                Literal::new_simple_literal(replication_factor.to_string()).into(),
            )?;
            self.insert_triple(
                resource_iri_str,
                &format!("{PICLOUD_NS}replicationCount"),
                Literal::new_simple_literal(replication_factor.to_string()).into(),
            )?;

            // Snapshot and backup status placeholders (ADR-047)
            self.insert_triple(
                resource_iri_str,
                &format!("{PICLOUD_NS}lastSnapshotStatus"),
                Literal::new_simple_literal("none").into(),
            )?;
            self.insert_triple(
                resource_iri_str,
                &format!("{PICLOUD_NS}lastBackupStatus"),
                Literal::new_simple_literal("none").into(),
            )?;
        }

        // Project ingress-specific triples (ADR-048)
        if resource_type == "Ingress" || resource_type == "ingress" {
            self.insert_triple(
                resource_iri_str,
                RDF_TYPE,
                picloud_term("Ingress").into(),
            )?;
            if let Some(name) = event.payload["name"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}name"),
                    Literal::new_simple_literal(name).into(),
                )?;
            }
            if let Some(hostname) = event.payload["hostname"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}hostname"),
                    Literal::new_simple_literal(hostname).into(),
                )?;
            }
            if let Some(host) = event.payload["host"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}hostname"),
                    Literal::new_simple_literal(host).into(),
                )?;
            }
            if let Some(target) = event.payload["target"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}targetAddress"),
                    Literal::new_simple_literal(target).into(),
                )?;
            }
            if let Some(port) = event.payload.get("port") {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}port"),
                    Literal::new_simple_literal(&port.to_string()).into(),
                )?;
            }
        }

        // Project container-specific triples (ADR-008)
        if resource_type == "Container" || resource_type == "container" {
            self.insert_triple(
                resource_iri_str,
                RDF_TYPE,
                picloud_term("Container").into(),
            )?;
            if let Some(name) = event.payload["name"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}name"),
                    Literal::new_simple_literal(name).into(),
                )?;
            }
            if let Some(image) = event.payload["image"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}image"),
                    Literal::new_simple_literal(image).into(),
                )?;
            }
        }

        // Project feature-flag-specific triples when declared via apply (ADR-044)
        if resource_type == "FeatureFlag" || resource_type == "feature-flag" {
            self.insert_triple(
                resource_iri_str,
                RDF_TYPE,
                picloud_term("FeatureFlag").into(),
            )?;
            if let Some(name) = event.payload["name"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}flagName"),
                    Literal::new_simple_literal(name).into(),
                )?;
            }
            let enabled = event.payload["enabled"].as_bool().unwrap_or(false);
            self.insert_triple(
                resource_iri_str,
                &format!("{PICLOUD_NS}flagEnabled"),
                Literal::new_simple_literal(if enabled { "true" } else { "false" }).into(),
            )?;
            if let Some(version) = event.payload["version"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}flagVersion"),
                    Literal::new_simple_literal(version).into(),
                )?;
            }
        }

        // Project inference-rule-specific triples (ADR-038)
        if resource_type == "InferenceRule" || resource_type == "inference-rule" {
            self.insert_triple(
                resource_iri_str,
                RDF_TYPE,
                picloud_term("InferenceRule").into(),
            )?;
            if let Some(scope) = event.payload["scope"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}scope"),
                    Literal::new_simple_literal(scope).into(),
                )?;
            }
            if let Some(construct) = event.payload["construct_query"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}constructQuery"),
                    Literal::new_simple_literal(construct).into(),
                )?;
            }
        }

        // Project NetworkPolicy-specific triples (ADR-028)
        if resource_type == "NetworkPolicy" {
            self.insert_triple(
                resource_iri_str,
                RDF_TYPE,
                picloud_term("NetworkPolicy").into(),
            )?;
            if let Some(isolation_type) = event.payload["isolation_type"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}isolationType"),
                    Literal::new_simple_literal(isolation_type).into(),
                )?;
            }
        }

        // Project EventStore-specific triples (ADR-032, FT-078)
        if resource_type == "EventStore" || resource_type == "event-store" {
            self.insert_triple(
                resource_iri_str,
                RDF_TYPE,
                picloud_term("EventStore").into(),
            )?;
            // Project aggregate definitions as blank-node-like sub-resources
            if let Some(aggregates) = event.payload["aggregates"].as_array() {
                for agg in aggregates {
                    if let Some(agg_type) = agg["aggregate_type"].as_str() {
                        // Link event store to aggregate type
                        self.insert_triple(
                            resource_iri_str,
                            &format!("{PICLOUD_NS}aggregateType"),
                            Literal::new_simple_literal(agg_type).into(),
                        )?;
                    }
                    if let Some(schema_file) = agg["schema_file"].as_str() {
                        self.insert_triple(
                            resource_iri_str,
                            &format!("{PICLOUD_NS}aggregateSchemaFile"),
                            Literal::new_simple_literal(schema_file).into(),
                        )?;
                    }
                }
            }
            // Also insert into named graph if product-scoped
            if let Some(product) = event.payload["product"].as_str() {
                let graph_iri = self.iri_builder.product_graph(product);
                self.insert_triple_in_graph(
                    resource_iri_str,
                    RDF_TYPE,
                    picloud_term("EventStore").into(),
                    graph_iri.as_str(),
                )?;
                // Project aggregate types into the named graph too
                if let Some(aggregates) = event.payload["aggregates"].as_array() {
                    for agg in aggregates {
                        if let Some(agg_type) = agg["aggregate_type"].as_str() {
                            self.insert_triple_in_graph(
                                resource_iri_str,
                                &format!("{PICLOUD_NS}aggregateType"),
                                Literal::new_simple_literal(agg_type).into(),
                                graph_iri.as_str(),
                            )?;
                        }
                    }
                }
            }
        }

        // Project EventSubscription-specific triples (ADR-018)
        // Project rdf-store-specific triples (ADR-019, FT-051)
        if resource_type == "RdfStore" || resource_type == "rdf-store" {
            self.insert_triple(
                resource_iri_str,
                RDF_TYPE,
                picloud_term("RdfStore").into(),
            )?;
            // sparqlEndpoint — the IRI where SPARQL queries are served
            if let Some(endpoint) = event.payload["sparql_endpoint"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}sparqlEndpoint"),
                    NamedNode::new(endpoint)
                        .map_err(|e| PiCloudError::Internal(format!("invalid SPARQL endpoint IRI: {e}")))?
                        .into(),
                )?;
            } else if let Some(product) = event.payload["product"].as_str() {
                // Derive the endpoint IRI from the product name
                let endpoint = self.iri_builder.product_sparql(product);
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}sparqlEndpoint"),
                    NamedNode::new(endpoint.as_str())
                        .map_err(|e| PiCloudError::Internal(format!("invalid SPARQL endpoint IRI: {e}")))?
                        .into(),
                )?;
            }
            // backingVolume — the block volume providing persistence
            if let Some(volume) = event.payload["backing_volume"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}backingVolume"),
                    NamedNode::new(volume)
                        .map_err(|e| PiCloudError::Internal(format!("invalid backing volume IRI: {e}")))?
                        .into(),
                )?;
            }
            // Also insert into named graph if product-scoped
            if let Some(product) = event.payload["product"].as_str() {
                let graph_iri = self.iri_builder.product_graph(product);
                self.insert_triple_in_graph(
                    resource_iri_str,
                    RDF_TYPE,
                    picloud_term("RdfStore").into(),
                    graph_iri.as_str(),
                )?;
            }
        }

        if resource_type == "EventSubscription" {
            self.insert_triple(
                resource_iri_str,
                RDF_TYPE,
                picloud_term("EventSubscription").into(),
            )?;
            if let Some(source) = event.payload["source_product"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}sourceProduct"),
                    Literal::new_simple_literal(source).into(),
                )?;
            }
            if let Some(event_type) = event.payload["event_type"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}eventType"),
                    Literal::new_simple_literal(event_type).into(),
                )?;
            }
            if let Some(handler) = event.payload["handler"].as_str() {
                if !handler.is_empty() {
                    self.insert_triple(
                        resource_iri_str,
                        &format!("{PICLOUD_NS}handler"),
                        Literal::new_simple_literal(handler).into(),
                    )?;
                }
            }
        }

        // Project Ontology-specific triples (ADR-023, FT-053)
        if resource_type == "Ontology" || resource_type == "ontology" {
            self.insert_triple(
                resource_iri_str,
                RDF_TYPE,
                picloud_term("Ontology").into(),
            )?;
            if let Some(file_path) = event.payload["file_path"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}filePath"),
                    Literal::new_simple_literal(file_path).into(),
                )?;
            }
            if let Some(format) = event.payload["format"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}format"),
                    Literal::new_simple_literal(format).into(),
                )?;
            }
            if let Some(served_at) = event.payload["served_at"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}servedAt"),
                    NamedNode::new(served_at)
                        .map_err(|e| PiCloudError::Internal(format!("invalid served_at IRI: {e}")))?
                        .into(),
                )?;
            }
            if let Some(version) = event.payload["version"].as_str() {
                self.insert_triple(
                    resource_iri_str,
                    &format!("{PICLOUD_NS}version"),
                    Literal::new_simple_literal(version).into(),
                )?;
            }
            // Also insert typed triple into product named graph
            if let Some(product) = event.payload["product"].as_str() {
                let graph_iri = self.iri_builder.product_graph(product);
                self.insert_triple_in_graph(
                    resource_iri_str,
                    RDF_TYPE,
                    picloud_term("Ontology").into(),
                    graph_iri.as_str(),
                )?;
            }
        }

        debug!(resource_iri = resource_iri_str, "projected ResourceDeclared");
        Ok(())
    }

    fn project_resource_provisioning(&self, event: &EventEnvelope) -> Result<()> {
        let resource_iri_str = event.payload["resource_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());

        self.update_status(resource_iri_str, "provisioning", event.product.as_deref())?;

        // Add provisioning message if present
        if let Some(message) = event.payload["message"].as_str() {
            self.insert_triple(
                resource_iri_str,
                &format!("{PICLOUD_NS}provisioningMessage"),
                Literal::new_simple_literal(message).into(),
            )?;
        }

        debug!(resource_iri = resource_iri_str, "projected ResourceProvisioning");
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

        // ADR-023: Project versioned ontology IRI
        let ontology_iri = self.iri_builder.product_ontology_versioned(product_name, version);
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}ontologyIri"),
            NamedNode::new(ontology_iri.as_str())
                .map_err(|e| PiCloudError::Internal(e.to_string()))?
                .into(),
        )?;

        // ADR-028: Default product network isolation policy
        let policy_iri = format!("{}/network-policy", product_iri_str);
        self.insert_triple(
            &policy_iri,
            RDF_TYPE,
            picloud_term("NetworkPolicy").into(),
        )?;
        self.insert_triple(
            &policy_iri,
            &format!("{PICLOUD_NS}isolationType"),
            Literal::new_simple_literal("product").into(),
        )?;
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}networkPolicy"),
            NamedNode::new(&policy_iri)
                .map_err(|e| PiCloudError::Internal(e.to_string()))?
                .into(),
        )?;

        debug!(product_iri = product_iri_str, "projected ProductDeployed");
        Ok(())
    }

    /// Project an OntologyLoaded event (ADR-023, FT-053).
    ///
    /// Loads the ontology file content (Turtle or SHACL) into the product's
    /// named graph and materialises RDFS inference. Links the ontology
    /// resource to the versioned ontology IRI.
    fn project_ontology_loaded(&self, event: &EventEnvelope) -> Result<()> {
        let ontology_iri_str = event.payload["ontology_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let product = event.payload["product"]
            .as_str()
            .unwrap_or_default();
        let version = event.payload["version"]
            .as_str()
            .unwrap_or("0.0.0");
        let content = event.payload["content"]
            .as_str()
            .unwrap_or("");
        let format = event.payload["format"]
            .as_str()
            .unwrap_or("turtle");

        if content.is_empty() {
            debug!(ontology_iri = ontology_iri_str, "OntologyLoaded with empty content — skipping");
            return Ok(());
        }

        // Load content into the product's named graph
        let graph_iri = self.iri_builder.product_graph(product);
        self.load_ontology(content, Some(graph_iri.as_str()))?;

        // Also load into default graph so platform-wide queries can find the triples
        self.load_ontology(content, None)?;

        // Bind ontology to versioned IRI
        let versioned_iri = self.iri_builder.product_ontology_versioned(product, version);
        self.insert_triple(
            ontology_iri_str,
            &format!("{PICLOUD_NS}boundToVersion"),
            NamedNode::new(versioned_iri.as_str())
                .map_err(|e| PiCloudError::Internal(e.to_string()))?
                .into(),
        )?;

        // Record the format
        self.insert_triple(
            ontology_iri_str,
            &format!("{PICLOUD_NS}loadedFormat"),
            Literal::new_simple_literal(format).into(),
        )?;

        // Mark ontology as loaded (status → ready)
        self.update_status(ontology_iri_str, "ready", Some(product))?;

        debug!(
            ontology_iri = ontology_iri_str,
            product = product,
            version = version,
            format = format,
            "projected OntologyLoaded — content loaded into product graph"
        );
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

    fn project_product_deleted(&self, event: &EventEnvelope) -> Result<()> {
        let product_iri_str = event.payload["product_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());

        // Remove the product resource itself from all graphs
        self.remove_triples_about_all_graphs(product_iri_str)?;

        // Also remove the product's named graph entirely — all child resources
        // that were projected into it disappear with it.
        if let Some(product_name) = event.product.as_deref() {
            let graph_iri = self.iri_builder.product_graph(product_name);
            // Remove all triples in the named graph
            let g = NamedNode::new(graph_iri.as_str())
                .map_err(|e| PiCloudError::Internal(format!("invalid graph IRI: {e}")))?;
            let quads: Vec<Quad> = self
                .store
                .quads_for_pattern(None, None, None, Some(GraphNameRef::from(&g)))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| PiCloudError::Internal(e.to_string()))?;
            for quad in &quads {
                self.store
                    .remove(quad)
                    .map_err(|e| PiCloudError::Internal(e.to_string()))?;
            }
        }

        // Also remove child resources from the default graph.
        // First, try children linked via picloud:product literal.
        // Then also find children whose IRI contains the product path segment.
        if let Some(product_name) = event.product.as_deref()
            .or_else(|| event.payload["product_iri"].as_str()
                .and_then(|iri| iri.rsplit('/').next()))
        {
            // Query 1: children linked via picloud:product predicate
            let child_query = format!(
                "SELECT ?child WHERE {{ ?child <{PICLOUD_NS}product> \"{}\" }}", product_name
            );
            if let Ok(result) = self.execute_query(&child_query) {
                for row in &result.bindings {
                    if let Some(child_iri) = row.get("child").and_then(|v| v.get("value")).and_then(|v| v.as_str()) {
                        let _ = self.remove_triples_about_all_graphs(child_iri);
                    }
                }
            }

            // Query 2: any subject whose IRI contains /products/{product_name}/
            // (catches NetworkPolicy, ontology, and other auto-created resources
            // that may not be typed as picloud:Resource)
            let path_query = format!(
                "SELECT DISTINCT ?child WHERE {{ ?child ?p ?o . FILTER(CONTAINS(STR(?child), \"/products/{}/\")) }}",
                product_name
            );
            if let Ok(result) = self.execute_query(&path_query) {
                for row in &result.bindings {
                    if let Some(child_iri) = row.get("child").and_then(|v| v.get("value")).and_then(|v| v.as_str()) {
                        let _ = self.remove_triples_about_all_graphs(child_iri);
                    }
                }
            }
        }

        debug!(product_iri = product_iri_str, "projected ProductDeleted");
        Ok(())
    }

    /// Project a ProductEventAppended event (ADR-032, FT-078).
    ///
    /// When a product appends an event to its event store, this projector
    /// creates RDF triples in the product's named graph representing the
    /// event and its payload properties.
    fn project_product_event_appended(&self, event: &EventEnvelope) -> Result<()> {
        let product_name = event.product.as_deref().ok_or_else(|| {
            PiCloudError::Internal("ProductEventAppended missing product scope".to_string())
        })?;
        let graph_iri = self.iri_builder.product_graph(product_name);

        // The event IRI is the aggregate stream address for this event
        let event_iri = event.payload["event_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let aggregate_type = event.payload["aggregate_type"]
            .as_str()
            .unwrap_or("Unknown");
        let aggregate_id = event.payload["aggregate_id"]
            .as_str()
            .unwrap_or("unknown");
        let product_event_type = event.payload["product_event_type"]
            .as_str()
            .unwrap_or("Event");

        // Insert into default graph
        self.insert_triple(
            event_iri,
            RDF_TYPE,
            picloud_term("ProductEvent").into(),
        )?;
        self.insert_triple(
            event_iri,
            &format!("{PICLOUD_NS}aggregateType"),
            Literal::new_simple_literal(aggregate_type).into(),
        )?;
        self.insert_triple(
            event_iri,
            &format!("{PICLOUD_NS}aggregateId"),
            Literal::new_simple_literal(aggregate_id).into(),
        )?;
        self.insert_triple(
            event_iri,
            &format!("{PICLOUD_NS}productEventType"),
            Literal::new_simple_literal(product_event_type).into(),
        )?;
        self.insert_triple(
            event_iri,
            &format!("{PICLOUD_NS}product"),
            Literal::new_simple_literal(product_name).into(),
        )?;

        // Project payload data properties into the graph
        if let Some(data) = event.payload.get("data") {
            if let Some(obj) = data.as_object() {
                for (key, value) in obj {
                    let literal_value = match value {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        _ => serde_json::to_string(value).unwrap_or_default(),
                    };
                    self.insert_triple(
                        event_iri,
                        &format!("{PICLOUD_NS}{key}"),
                        Literal::new_simple_literal(&literal_value).into(),
                    )?;
                }
            }
        }

        // Insert into product named graph
        self.insert_triple_in_graph(
            event_iri,
            RDF_TYPE,
            picloud_term("ProductEvent").into(),
            graph_iri.as_str(),
        )?;
        self.insert_triple_in_graph(
            event_iri,
            &format!("{PICLOUD_NS}aggregateType"),
            Literal::new_simple_literal(aggregate_type).into(),
            graph_iri.as_str(),
        )?;
        self.insert_triple_in_graph(
            event_iri,
            &format!("{PICLOUD_NS}aggregateId"),
            Literal::new_simple_literal(aggregate_id).into(),
            graph_iri.as_str(),
        )?;
        self.insert_triple_in_graph(
            event_iri,
            &format!("{PICLOUD_NS}productEventType"),
            Literal::new_simple_literal(product_event_type).into(),
            graph_iri.as_str(),
        )?;

        // Project payload data into product named graph too
        if let Some(data) = event.payload.get("data") {
            if let Some(obj) = data.as_object() {
                for (key, value) in obj {
                    let literal_value = match value {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        _ => serde_json::to_string(value).unwrap_or_default(),
                    };
                    self.insert_triple_in_graph(
                        event_iri,
                        &format!("{PICLOUD_NS}{key}"),
                        Literal::new_simple_literal(&literal_value).into(),
                        graph_iri.as_str(),
                    )?;
                }
            }
        }

        debug!(
            product = product_name,
            aggregate_type = aggregate_type,
            aggregate_id = aggregate_id,
            "projected ProductEventAppended"
        );
        Ok(())
    }

    /// Project a ProductUpgradeStarted event (ADR-017).
    fn project_product_upgrade_started(&self, event: &EventEnvelope) -> Result<()> {
        let product_iri_str = event.payload["product_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let from_version = event.payload["from_version"]
            .as_str()
            .unwrap_or("unknown");
        let to_version = event.payload["to_version"]
            .as_str()
            .unwrap_or("unknown");

        // Set upgrade state
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}upgradeState"),
            Literal::new_simple_literal("Provisioning").into(),
        )?;
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}upgradeFromVersion"),
            Literal::new_simple_literal(from_version).into(),
        )?;
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}upgradeToVersion"),
            Literal::new_simple_literal(to_version).into(),
        )?;
        // Update status
        self.remove_triple(product_iri_str, &format!("{PICLOUD_NS}status"))?;
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}status"),
            Literal::new_simple_literal("upgrading").into(),
        )?;

        debug!(product_iri = product_iri_str, to_version, "projected ProductUpgradeStarted");
        Ok(())
    }

    /// Project a ProductUpgradeCompleted event (ADR-017).
    fn project_product_upgrade_completed(&self, event: &EventEnvelope) -> Result<()> {
        let product_iri_str = event.payload["product_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let version = event.payload["to_version"]
            .as_str()
            .or_else(|| event.payload["version"].as_str())
            .unwrap_or("unknown");

        // Update version
        self.remove_triple(product_iri_str, &format!("{PICLOUD_NS}version"))?;
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}version"),
            Literal::new_simple_literal(version).into(),
        )?;
        // Clear upgrade state
        self.remove_triple(product_iri_str, &format!("{PICLOUD_NS}upgradeState"))?;
        self.remove_triple(product_iri_str, &format!("{PICLOUD_NS}upgradeFromVersion"))?;
        self.remove_triple(product_iri_str, &format!("{PICLOUD_NS}upgradeToVersion"))?;
        // Set status back to deployed
        self.remove_triple(product_iri_str, &format!("{PICLOUD_NS}status"))?;
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}status"),
            Literal::new_simple_literal("deployed").into(),
        )?;

        debug!(product_iri = product_iri_str, version, "projected ProductUpgradeCompleted");
        Ok(())
    }

    /// Project a ProductUpgradeAborted event (ADR-017).
    fn project_product_upgrade_aborted(&self, event: &EventEnvelope) -> Result<()> {
        let product_iri_str = event.payload["product_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let reason = event.payload["reason"]
            .as_str()
            .unwrap_or("unknown");

        // Set upgrade state to failed
        self.remove_triple(product_iri_str, &format!("{PICLOUD_NS}upgradeState"))?;
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}upgradeState"),
            Literal::new_simple_literal("Failed").into(),
        )?;
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}upgradeFailureReason"),
            Literal::new_simple_literal(reason).into(),
        )?;
        // Set status
        self.remove_triple(product_iri_str, &format!("{PICLOUD_NS}status"))?;
        self.insert_triple(
            product_iri_str,
            &format!("{PICLOUD_NS}status"),
            Literal::new_simple_literal("upgrade_failed").into(),
        )?;

        debug!(product_iri = product_iri_str, reason, "projected ProductUpgradeAborted");
        Ok(())
    }

    /// Project a MetricRecorded event (ADR-040).
    ///
    /// Upserts the latest metric values as triples on the node IRI.
    /// Previous metric triples for this node are removed first so the
    /// graph always holds only the current values.
    fn project_metric_recorded(&self, event: &EventEnvelope) -> Result<()> {
        let node_iri_str = event.payload["node_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());

        let metrics = match event.payload["metrics"].as_array() {
            Some(arr) => arr,
            None => {
                warn!("MetricRecorded event missing metrics array");
                return Ok(());
            }
        };

        // Map metric names to RDF predicate local names
        let metric_predicates: &[(&str, &str)] = &[
            ("cpu_usage_percent", "cpuUsagePercent"),
            ("memory_used_mb", "memoryUsedMb"),
            ("memory_total_mb", "memoryTotalMb"),
            ("disk_used_gb", "diskUsedGb"),
            ("disk_total_gb", "diskTotalGb"),
            ("cpu_temp_celsius", "cpuTempCelsius"),
        ];

        // Remove old metric triples for this node (upsert pattern)
        let s = NamedNode::new(node_iri_str)
            .map_err(|e| PiCloudError::Internal(format!("invalid node IRI: {e}")))?;

        for &(_, predicate_local) in metric_predicates {
            let pred = format!("{PICLOUD_NS}{predicate_local}");
            let p = NamedNodeRef::new(&pred)
                .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;
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
        }

        // Also remove old metricsUpdatedAt triple
        {
            let pred = format!("{PICLOUD_NS}metricsUpdatedAt");
            let p = NamedNodeRef::new(&pred)
                .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;
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
        }

        // Insert new metric triples
        for metric in metrics {
            let name = metric["name"].as_str().unwrap_or_default();
            let value = metric["value"].as_f64().unwrap_or(0.0);

            // Find the matching predicate local name
            if let Some(&(_, predicate_local)) = metric_predicates.iter().find(|&&(n, _)| n == name) {
                self.insert_triple(
                    node_iri_str,
                    &format!("{PICLOUD_NS}{predicate_local}"),
                    Literal::new_typed_literal(
                        value.to_string(),
                        NamedNode::new("http://www.w3.org/2001/XMLSchema#double")
                            .expect("valid XSD IRI"),
                    )
                    .into(),
                )?;
            }
        }

        // Insert metricsUpdatedAt timestamp
        self.insert_triple(
            node_iri_str,
            &format!("{PICLOUD_NS}metricsUpdatedAt"),
            Literal::new_typed_literal(
                event.timestamp.to_rfc3339(),
                NamedNode::new("http://www.w3.org/2001/XMLSchema#dateTime")
                    .expect("valid XSD IRI"),
            )
            .into(),
        )?;

        debug!(node_iri = node_iri_str, "projected MetricRecorded");
        Ok(())
    }

    /// Project a TagAdded event — inserts blank-node tag triples (ADR-036).
    ///
    /// RDF representation:
    /// ```turtle
    /// <resource> picloud:tag [ picloud:tagKey "key" ; picloud:tagValue "value" ] .
    /// ```
    fn project_tag_added(&self, event: &EventEnvelope) -> Result<()> {
        let resource_iri_str = event.payload["resource_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let key = event.payload["key"]
            .as_str()
            .unwrap_or_default();
        let value = event.payload["value"]
            .as_str()
            .unwrap_or_default();

        // Use a deterministic blank node ID based on resource + key + value
        // so that removal can find it.
        let bnode_id = format!("tag-{}-{}-{}", resource_iri_str, key, value);
        let bnode_id_safe = bnode_id
            .replace("://", "-")
            .replace('/', "-")
            .replace('.', "-")
            .replace(':', "-");
        let bnode = oxigraph::model::BlankNode::new(&bnode_id_safe)
            .map_err(|e| PiCloudError::Internal(format!("invalid bnode id: {e}")))?;

        let subject = NamedNode::new(resource_iri_str)
            .map_err(|e| PiCloudError::Internal(format!("invalid subject IRI: {e}")))?;
        let tag_pred = NamedNode::new(format!("{PICLOUD_NS}tag"))
            .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;
        let tag_key_pred = NamedNode::new(format!("{PICLOUD_NS}tagKey"))
            .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;
        let tag_value_pred = NamedNode::new(format!("{PICLOUD_NS}tagValue"))
            .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;

        // <resource> picloud:tag _:bnode
        self.store
            .insert(QuadRef::new(
                &subject,
                &tag_pred,
                &bnode,
                GraphNameRef::DefaultGraph,
            ))
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;

        // _:bnode picloud:tagKey "key"
        self.store
            .insert(QuadRef::new(
                &bnode,
                &tag_key_pred,
                &Literal::new_simple_literal(key),
                GraphNameRef::DefaultGraph,
            ))
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;

        // _:bnode picloud:tagValue "value"
        self.store
            .insert(QuadRef::new(
                &bnode,
                &tag_value_pred,
                &Literal::new_simple_literal(value),
                GraphNameRef::DefaultGraph,
            ))
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;

        // If product-scoped, also insert into the product's named graph
        if let Some(product) = &event.product {
            let graph_iri = self.iri_builder.product_graph(product);
            let g = NamedNode::new(graph_iri.as_str())
                .map_err(|e| PiCloudError::Internal(format!("invalid graph IRI: {e}")))?;
            self.store
                .insert_named_graph(NamedNodeRef::new(graph_iri.as_str()).unwrap())
                .map_err(|e| PiCloudError::Internal(e.to_string()))?;

            self.store
                .insert(QuadRef::new(&subject, &tag_pred, &bnode, &g))
                .map_err(|e| PiCloudError::Internal(e.to_string()))?;
            self.store
                .insert(QuadRef::new(
                    &bnode,
                    &tag_key_pred,
                    &Literal::new_simple_literal(key),
                    &g,
                ))
                .map_err(|e| PiCloudError::Internal(e.to_string()))?;
            self.store
                .insert(QuadRef::new(
                    &bnode,
                    &tag_value_pred,
                    &Literal::new_simple_literal(value),
                    &g,
                ))
                .map_err(|e| PiCloudError::Internal(e.to_string()))?;
        }

        debug!(
            resource_iri = resource_iri_str,
            tag_key = key,
            tag_value = value,
            "projected TagAdded"
        );
        Ok(())
    }

    /// Project a TagRemoved event — removes matching tag blank-node triples (ADR-036).
    fn project_tag_removed(&self, event: &EventEnvelope) -> Result<()> {
        let resource_iri_str = event.payload["resource_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let key = event.payload["key"]
            .as_str()
            .unwrap_or_default();
        let value = event.payload["value"]
            .as_str()
            .unwrap_or_default();

        // Reconstruct the deterministic blank node ID
        let bnode_id = format!("tag-{}-{}-{}", resource_iri_str, key, value);
        let bnode_id_safe = bnode_id
            .replace("://", "-")
            .replace('/', "-")
            .replace('.', "-")
            .replace(':', "-");
        let bnode = oxigraph::model::BlankNode::new(&bnode_id_safe)
            .map_err(|e| PiCloudError::Internal(format!("invalid bnode id: {e}")))?;

        // Remove all triples about the blank node (in all graphs)
        let quads: Vec<Quad> = self
            .store
            .quads_for_pattern(Some(SubjectRef::from(&bnode)), None, None, None)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;
        for quad in &quads {
            self.store
                .remove(quad)
                .map_err(|e| PiCloudError::Internal(e.to_string()))?;
        }

        // Remove the <resource> picloud:tag _:bnode triple (in all graphs)
        let subject = NamedNode::new(resource_iri_str)
            .map_err(|e| PiCloudError::Internal(format!("invalid subject IRI: {e}")))?;
        let tag_pred = NamedNode::new(format!("{PICLOUD_NS}tag"))
            .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;
        let link_quads: Vec<Quad> = self
            .store
            .quads_for_pattern(
                Some(SubjectRef::from(&subject)),
                Some(NamedNodeRef::new(tag_pred.as_str()).unwrap()),
                Some(oxigraph::model::TermRef::from(&bnode)),
                None,
            )
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;
        for quad in &link_quads {
            self.store
                .remove(quad)
                .map_err(|e| PiCloudError::Internal(e.to_string()))?;
        }

        debug!(
            resource_iri = resource_iri_str,
            tag_key = key,
            tag_value = value,
            "projected TagRemoved"
        );
        Ok(())
    }

    /// Project a ConfigChanged event — insert/update/delete config entry triples (ADR-043).
    fn project_config_changed(&self, event: &EventEnvelope) -> Result<()> {
        let config_iri_str = event.payload["config_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let key = event.payload["key"]
            .as_str()
            .unwrap_or_default();
        let action = event.payload["action"]
            .as_str()
            .unwrap_or("set");
        let product = event.product.as_deref();

        if action == "deleted" {
            self.remove_triples_about_all_graphs(config_iri_str)?;
            debug!(config_iri = config_iri_str, key = key, "projected ConfigChanged (deleted)");
            return Ok(());
        }

        let value = event.payload["value"]
            .as_str()
            .unwrap_or_default();
        let config_type = event.payload["config_type"]
            .as_str()
            .unwrap_or("string");

        // Remove existing triples for this entry (in case of update)
        self.remove_triples_about_all_graphs(config_iri_str)?;

        // Insert config entry triples
        self.insert_triple(
            config_iri_str,
            RDF_TYPE,
            picloud_term("ConfigEntry").into(),
        )?;
        self.insert_triple(
            config_iri_str,
            &format!("{PICLOUD_NS}configKey"),
            Literal::new_simple_literal(key).into(),
        )?;
        self.insert_triple(
            config_iri_str,
            &format!("{PICLOUD_NS}configValue"),
            Literal::new_simple_literal(value).into(),
        )?;
        self.insert_triple(
            config_iri_str,
            &format!("{PICLOUD_NS}configType"),
            Literal::new_simple_literal(config_type).into(),
        )?;

        if let Some(product_name) = product {
            let graph_iri = self.iri_builder.product_graph(product_name);
            self.insert_triple_in_graph(
                config_iri_str,
                RDF_TYPE,
                picloud_term("ConfigEntry").into(),
                graph_iri.as_str(),
            )?;
            self.insert_triple_in_graph(
                config_iri_str,
                &format!("{PICLOUD_NS}configKey"),
                Literal::new_simple_literal(key).into(),
                graph_iri.as_str(),
            )?;
            self.insert_triple_in_graph(
                config_iri_str,
                &format!("{PICLOUD_NS}configValue"),
                Literal::new_simple_literal(value).into(),
                graph_iri.as_str(),
            )?;
            self.insert_triple_in_graph(
                config_iri_str,
                &format!("{PICLOUD_NS}configType"),
                Literal::new_simple_literal(config_type).into(),
                graph_iri.as_str(),
            )?;
        }

        debug!(config_iri = config_iri_str, key = key, "projected ConfigChanged (set)");
        Ok(())
    }

    /// Project a FeatureFlagChanged event — insert/update/delete flag triples (ADR-044).
    fn project_feature_flag_changed(&self, event: &EventEnvelope) -> Result<()> {
        let flag_iri_str = event.payload["flag_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let flag_name = event.payload["flag_name"]
            .as_str()
            .unwrap_or_default();
        let action = event.payload["action"]
            .as_str()
            .unwrap_or("set");
        let product = event.product.as_deref();

        if action == "deleted" {
            self.remove_triples_about_all_graphs(flag_iri_str)?;
            debug!(flag_iri = flag_iri_str, name = flag_name, "projected FeatureFlagChanged (deleted)");
            return Ok(());
        }

        let enabled = event.payload["enabled"].as_bool().unwrap_or(false);
        let version_expr = event.payload["version_expr"]
            .as_str()
            .or_else(|| event.payload["version"].as_str())
            .unwrap_or("");
        let description = event.payload["description"]
            .as_str()
            .unwrap_or_default();

        self.remove_triples_about_all_graphs(flag_iri_str)?;

        self.insert_triple(
            flag_iri_str,
            RDF_TYPE,
            picloud_term("FeatureFlag").into(),
        )?;
        self.insert_triple(
            flag_iri_str,
            &format!("{PICLOUD_NS}flagName"),
            Literal::new_simple_literal(flag_name).into(),
        )?;
        self.insert_triple(
            flag_iri_str,
            &format!("{PICLOUD_NS}flagEnabled"),
            Literal::new_simple_literal(if enabled { "true" } else { "false" }).into(),
        )?;
        self.insert_triple(
            flag_iri_str,
            &format!("{PICLOUD_NS}flagVersion"),
            Literal::new_simple_literal(version_expr).into(),
        )?;
        if !description.is_empty() {
            self.insert_triple(
                flag_iri_str,
                &format!("{PICLOUD_NS}flagDescription"),
                Literal::new_simple_literal(description).into(),
            )?;
        }

        if let Some(product_name) = product {
            let graph_iri = self.iri_builder.product_graph(product_name);
            self.insert_triple_in_graph(
                flag_iri_str,
                RDF_TYPE,
                picloud_term("FeatureFlag").into(),
                graph_iri.as_str(),
            )?;
            self.insert_triple_in_graph(
                flag_iri_str,
                &format!("{PICLOUD_NS}flagName"),
                Literal::new_simple_literal(flag_name).into(),
                graph_iri.as_str(),
            )?;
            self.insert_triple_in_graph(
                flag_iri_str,
                &format!("{PICLOUD_NS}flagEnabled"),
                Literal::new_simple_literal(if enabled { "true" } else { "false" }).into(),
                graph_iri.as_str(),
            )?;
            self.insert_triple_in_graph(
                flag_iri_str,
                &format!("{PICLOUD_NS}flagVersion"),
                Literal::new_simple_literal(version_expr).into(),
                graph_iri.as_str(),
            )?;
            if !description.is_empty() {
                self.insert_triple_in_graph(
                    flag_iri_str,
                    &format!("{PICLOUD_NS}flagDescription"),
                    Literal::new_simple_literal(description).into(),
                    graph_iri.as_str(),
                )?;
            }
        }

        debug!(flag_iri = flag_iri_str, name = flag_name, "projected FeatureFlagChanged (set)");
        Ok(())
    }

    fn project_group_membership_changed(&self, event: &EventEnvelope) -> Result<()> {
        let group_iri_str = event.payload["group_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let member_iri_str = event.payload["member_iri"]
            .as_str()
            .unwrap_or_default();
        let action = event.payload["action"]
            .as_str()
            .unwrap_or("added");

        match action {
            "added" => {
                // Ensure group type is declared
                self.insert_triple(
                    group_iri_str,
                    RDF_TYPE,
                    picloud_term("Group").into(),
                )?;
                self.insert_triple(
                    group_iri_str,
                    &format!("{PICLOUD_NS}hasMember"),
                    NamedNode::new(member_iri_str)
                        .map_err(|e| PiCloudError::Internal(format!("invalid member IRI: {e}")))?
                        .into(),
                )?;
                debug!(group = group_iri_str, member = member_iri_str, "projected GroupMembershipChanged (added)");
            }
            "removed" => {
                // Remove the specific hasMember triple
                let s = NamedNode::new(group_iri_str)
                    .map_err(|e| PiCloudError::Internal(format!("invalid group IRI: {e}")))?;
                let p = NamedNode::new(format!("{PICLOUD_NS}hasMember"))
                    .map_err(|e| PiCloudError::Internal(format!("invalid predicate: {e}")))?;
                let o = NamedNode::new(member_iri_str)
                    .map_err(|e| PiCloudError::Internal(format!("invalid member IRI: {e}")))?;
                let quad = Quad::new(s, p, o, oxigraph::model::GraphName::DefaultGraph);
                let _ = self.store.remove(&quad);
                debug!(group = group_iri_str, member = member_iri_str, "projected GroupMembershipChanged (removed)");
            }
            _ => {
                debug!(action = action, "unknown GroupMembershipChanged action — skipping");
            }
        }
        Ok(())
    }

    // ---- OCI Registry Projection (ADR-054) ----

    /// Project an ImagePushed event — create repository, tag, and image triples.
    fn project_image_pushed(&self, event: &EventEnvelope) -> Result<()> {
        let repository = event.payload["repository"]
            .as_str()
            .unwrap_or_default();
        let digest = event.payload["digest"]
            .as_str()
            .unwrap_or_default();
        let size_bytes = event.payload["size_bytes"]
            .as_u64()
            .unwrap_or(0);
        let media_type = event.payload["media_type"]
            .as_str()
            .unwrap_or_default();
        let pushed_by = event.payload["pushed_by"]
            .as_str()
            .unwrap_or_default();

        let cluster_domain = &self.iri_builder.cluster_root();
        let base = cluster_domain.as_str().trim_end_matches('/');

        // Repository IRI: {base}/registry/{repository}
        let repo_iri = format!("{base}/registry/{repository}");
        // Image IRI: {base}/registry/{repository}/digests/{digest}
        let image_iri = format!("{base}/registry/{repository}/digests/{digest}");

        // Ensure the repository resource exists
        self.insert_triple(
            &repo_iri,
            RDF_TYPE,
            picloud_term("Repository").into(),
        )?;
        self.insert_triple(
            &repo_iri,
            &format!("{PICLOUD_NS}repositoryName"),
            Literal::new_simple_literal(repository).into(),
        )?;

        // Create the image triples
        self.insert_triple(
            &image_iri,
            RDF_TYPE,
            picloud_term("Image").into(),
        )?;
        self.insert_triple(
            &image_iri,
            &format!("{PICLOUD_NS}digest"),
            Literal::new_simple_literal(digest).into(),
        )?;
        self.insert_triple(
            &image_iri,
            &format!("{PICLOUD_NS}sizeBytes"),
            Literal::new_typed_literal(
                size_bytes.to_string(),
                NamedNode::new("http://www.w3.org/2001/XMLSchema#unsignedLong")
                    .expect("valid XSD IRI"),
            )
            .into(),
        )?;
        self.insert_triple(
            &image_iri,
            &format!("{PICLOUD_NS}mediaType"),
            Literal::new_simple_literal(media_type).into(),
        )?;
        if !pushed_by.is_empty() {
            self.insert_triple(
                &image_iri,
                &format!("{PICLOUD_NS}pushedBy"),
                Literal::new_simple_literal(pushed_by).into(),
            )?;
        }
        self.insert_triple(
            &image_iri,
            &format!("{PICLOUD_NS}pushedAt"),
            Literal::new_typed_literal(
                event.timestamp.to_rfc3339(),
                NamedNode::new("http://www.w3.org/2001/XMLSchema#dateTime")
                    .expect("valid XSD IRI"),
            )
            .into(),
        )?;

        // Link repository to image
        self.insert_triple(
            &repo_iri,
            &format!("{PICLOUD_NS}hasImage"),
            NamedNode::new(&image_iri)
                .map_err(|e| PiCloudError::Internal(format!("invalid image IRI: {e}")))?
                .into(),
        )?;

        // If a tag is provided, create tag triple
        if let Some(tag) = event.payload["tag"].as_str() {
            if !tag.is_empty() {
                let tag_iri = format!("{base}/registry/{repository}/tags/{tag}");
                self.insert_triple(
                    &tag_iri,
                    RDF_TYPE,
                    picloud_term("ImageTag").into(),
                )?;
                self.insert_triple(
                    &tag_iri,
                    &format!("{PICLOUD_NS}tagName"),
                    Literal::new_simple_literal(tag).into(),
                )?;
                self.insert_triple(
                    &tag_iri,
                    &format!("{PICLOUD_NS}pointsToDigest"),
                    NamedNode::new(&image_iri)
                        .map_err(|e| PiCloudError::Internal(format!("invalid image IRI: {e}")))?
                        .into(),
                )?;
                self.insert_triple(
                    &repo_iri,
                    &format!("{PICLOUD_NS}hasTag"),
                    NamedNode::new(&tag_iri)
                        .map_err(|e| PiCloudError::Internal(format!("invalid tag IRI: {e}")))?
                        .into(),
                )?;
            }
        }

        debug!(repository = repository, digest = digest, "projected ImagePushed");
        Ok(())
    }

    /// Project an ImageDeleted event — remove triples for the deleted image digest.
    fn project_image_deleted(&self, event: &EventEnvelope) -> Result<()> {
        let repository = event.payload["repository"]
            .as_str()
            .unwrap_or_default();
        let digest = event.payload["digest"]
            .as_str()
            .unwrap_or_default();

        let cluster_domain = &self.iri_builder.cluster_root();
        let base = cluster_domain.as_str().trim_end_matches('/');
        let repo_iri = format!("{base}/registry/{repository}");
        let image_iri = format!("{base}/registry/{repository}/digests/{digest}");

        // Remove the hasImage link from repo to this image
        let s = NamedNode::new(&repo_iri)
            .map_err(|e| PiCloudError::Internal(format!("invalid repo IRI: {e}")))?;
        let p = NamedNode::new(format!("{PICLOUD_NS}hasImage"))
            .map_err(|e| PiCloudError::Internal(format!("invalid predicate: {e}")))?;
        let o = NamedNode::new(&image_iri)
            .map_err(|e| PiCloudError::Internal(format!("invalid image IRI: {e}")))?;
        let quad = Quad::new(s, p, o, GraphName::DefaultGraph);
        let _ = self.store.remove(&quad);

        // Remove all triples about the image itself
        self.remove_triples_about(&image_iri)?;

        // Remove any tags that point to this digest
        let sparql = format!(
            "SELECT ?tag WHERE {{ ?tag <{PICLOUD_NS}pointsToDigest> <{image_iri}> }}"
        );
        if let Ok(result) = self.execute_query(&sparql) {
            for row in &result.bindings {
                if let Some(tag_iri) = row.get("tag").and_then(|v| v.get("value")).and_then(|v| v.as_str()) {
                    self.remove_triples_about(tag_iri)?;
                    // Remove hasTag link from repo
                    let rs = NamedNode::new(&repo_iri)
                        .map_err(|e| PiCloudError::Internal(format!("invalid repo IRI: {e}")))?;
                    let rp = NamedNode::new(format!("{PICLOUD_NS}hasTag"))
                        .map_err(|e| PiCloudError::Internal(format!("invalid predicate: {e}")))?;
                    let ro = NamedNode::new(tag_iri)
                        .map_err(|e| PiCloudError::Internal(format!("invalid tag IRI: {e}")))?;
                    let rquad = Quad::new(rs, rp, ro, GraphName::DefaultGraph);
                    let _ = self.store.remove(&rquad);
                }
            }
        }

        debug!(repository = repository, digest = digest, "projected ImageDeleted");
        Ok(())
    }

    /// Project an ImageTagUpdated event — move a tag to point to a new digest.
    fn project_image_tag_updated(&self, event: &EventEnvelope) -> Result<()> {
        let repository = event.payload["repository"]
            .as_str()
            .unwrap_or_default();
        let tag = event.payload["tag"]
            .as_str()
            .unwrap_or_default();
        let new_digest = event.payload["new_digest"]
            .as_str()
            .unwrap_or_default();

        let cluster_domain = &self.iri_builder.cluster_root();
        let base = cluster_domain.as_str().trim_end_matches('/');
        let tag_iri = format!("{base}/registry/{repository}/tags/{tag}");
        let new_image_iri = format!("{base}/registry/{repository}/digests/{new_digest}");

        // Remove old pointsToDigest triple from the tag
        let s = NamedNode::new(&tag_iri)
            .map_err(|e| PiCloudError::Internal(format!("invalid tag IRI: {e}")))?;
        let pred = format!("{PICLOUD_NS}pointsToDigest");
        let p = NamedNodeRef::new(&pred)
            .map_err(|e| PiCloudError::Internal(format!("invalid predicate IRI: {e}")))?;
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

        // Insert new pointsToDigest triple
        self.insert_triple(
            &tag_iri,
            &format!("{PICLOUD_NS}pointsToDigest"),
            NamedNode::new(&new_image_iri)
                .map_err(|e| PiCloudError::Internal(format!("invalid image IRI: {e}")))?
                .into(),
        )?;

        // Ensure the tag resource exists (in case it was created outside ImagePushed)
        self.insert_triple(
            &tag_iri,
            RDF_TYPE,
            picloud_term("ImageTag").into(),
        )?;
        self.insert_triple(
            &tag_iri,
            &format!("{PICLOUD_NS}tagName"),
            Literal::new_simple_literal(tag).into(),
        )?;

        debug!(repository = repository, tag = tag, new_digest = new_digest, "projected ImageTagUpdated");
        Ok(())
    }

    /// Project a RegistryGCStarted event — add GC activity triple with start time.
    fn project_registry_gc_started(&self, event: &EventEnvelope) -> Result<()> {
        let scheduled_at = event.payload["scheduled_at"]
            .as_str()
            .unwrap_or_default();

        let cluster_domain = &self.iri_builder.cluster_root();
        let base = cluster_domain.as_str().trim_end_matches('/');
        let gc_iri = format!("{base}/registry/gc/{}", event.id);

        self.insert_triple(
            &gc_iri,
            RDF_TYPE,
            picloud_term("RegistryGC").into(),
        )?;
        self.insert_triple(
            &gc_iri,
            &format!("{PICLOUD_NS}status"),
            Literal::new_simple_literal("running").into(),
        )?;
        self.insert_triple(
            &gc_iri,
            &format!("{PICLOUD_NS}scheduledAt"),
            Literal::new_typed_literal(
                scheduled_at.to_string(),
                NamedNode::new("http://www.w3.org/2001/XMLSchema#dateTime")
                    .expect("valid XSD IRI"),
            )
            .into(),
        )?;

        debug!(gc_iri = gc_iri, "projected RegistryGCStarted");
        Ok(())
    }

    /// Project a RegistryGCCompleted event — update GC activity with completion info.
    fn project_registry_gc_completed(&self, event: &EventEnvelope) -> Result<()> {
        let blobs_deleted = event.payload["blobs_deleted"]
            .as_u64()
            .unwrap_or(0);
        let bytes_reclaimed = event.payload["bytes_reclaimed"]
            .as_u64()
            .unwrap_or(0);
        let duration_ms = event.payload["duration_ms"]
            .as_u64()
            .unwrap_or(0);

        // The GC IRI is linked via correlation_id to the RegistryGCStarted event.
        // We look up the GC resource by its correlation_id.
        let cluster_domain = &self.iri_builder.cluster_root();
        let base = cluster_domain.as_str().trim_end_matches('/');

        // Find the GC resource by correlation_id
        let sparql = format!(
            "SELECT ?gc WHERE {{ ?gc <{RDF_TYPE}> <{PICLOUD_NS}RegistryGC> . ?gc <{PICLOUD_NS}status> \"running\" }}"
        );
        let gc_iri = if let Ok(result) = self.execute_query(&sparql) {
            result.bindings.first()
                .and_then(|row| row.get("gc"))
                .and_then(|v| v.get("value"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        };

        // Fall back to constructing a GC IRI from the event's correlation_id
        let gc_iri = gc_iri.unwrap_or_else(|| {
            format!("{base}/registry/gc/{}", event.correlation_id)
        });

        // Update status from "running" to "completed"
        self.update_status(&gc_iri, "completed", None)?;

        self.insert_triple(
            &gc_iri,
            &format!("{PICLOUD_NS}blobsDeleted"),
            Literal::new_typed_literal(
                blobs_deleted.to_string(),
                NamedNode::new("http://www.w3.org/2001/XMLSchema#unsignedLong")
                    .expect("valid XSD IRI"),
            )
            .into(),
        )?;
        self.insert_triple(
            &gc_iri,
            &format!("{PICLOUD_NS}bytesReclaimed"),
            Literal::new_typed_literal(
                bytes_reclaimed.to_string(),
                NamedNode::new("http://www.w3.org/2001/XMLSchema#unsignedLong")
                    .expect("valid XSD IRI"),
            )
            .into(),
        )?;
        self.insert_triple(
            &gc_iri,
            &format!("{PICLOUD_NS}durationMs"),
            Literal::new_typed_literal(
                duration_ms.to_string(),
                NamedNode::new("http://www.w3.org/2001/XMLSchema#unsignedLong")
                    .expect("valid XSD IRI"),
            )
            .into(),
        )?;
        self.insert_triple(
            &gc_iri,
            &format!("{PICLOUD_NS}completedAt"),
            Literal::new_typed_literal(
                event.timestamp.to_rfc3339(),
                NamedNode::new("http://www.w3.org/2001/XMLSchema#dateTime")
                    .expect("valid XSD IRI"),
            )
            .into(),
        )?;

        debug!(gc_iri = gc_iri, blobs_deleted = blobs_deleted, bytes_reclaimed = bytes_reclaimed, "projected RegistryGCCompleted");
        Ok(())
    }

    // ---- Snapshot & Backup Projection (ADR-047) ----

    /// Project a SnapshotCreated event — update volume snapshot status triples.
    fn project_snapshot_created(&self, event: &EventEnvelope) -> Result<()> {
        let volume_iri_str = event.payload["volume_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let snapshot_path = event.payload["snapshot_path"]
            .as_str()
            .unwrap_or_default();
        let ts_fallback2 = event.timestamp.to_rfc3339();
        let created_at = event.payload["created_at"]
            .as_str()
            .unwrap_or(&ts_fallback2);

        // Create a snapshot event IRI and mark it as SnapshotCreated type
        let snapshot_iri = format!("{}/snapshots/{}", volume_iri_str, created_at);
        self.insert_triple(
            &snapshot_iri,
            RDF_TYPE,
            picloud_term("SnapshotCreated").into(),
        )?;
        self.insert_triple(
            &snapshot_iri,
            RDF_TYPE,
            picloud_term("Snapshot").into(),
        )?;
        self.insert_triple(
            &snapshot_iri,
            &format!("{PICLOUD_NS}volume"),
            NamedNode::new(volume_iri_str)
                .map_err(|e| PiCloudError::Internal(format!("invalid volume IRI: {e}")))?
                .into(),
        )?;
        self.insert_triple(
            &snapshot_iri,
            &format!("{PICLOUD_NS}createdAt"),
            Literal::new_simple_literal(created_at).into(),
        )?;
        if !snapshot_path.is_empty() {
            self.insert_triple(
                &snapshot_iri,
                &format!("{PICLOUD_NS}snapshotPath"),
                Literal::new_simple_literal(snapshot_path).into(),
            )?;
        }

        // Update lastSnapshotAt and lastSnapshotStatus on the volume
        self.insert_triple(
            volume_iri_str,
            &format!("{PICLOUD_NS}lastSnapshotAt"),
            Literal::new_simple_literal(created_at).into(),
        )?;
        self.insert_triple(
            volume_iri_str,
            &format!("{PICLOUD_NS}lastSnapshotStatus"),
            Literal::new_simple_literal("success").into(),
        )?;
        self.insert_triple(
            volume_iri_str,
            &format!("{PICLOUD_NS}lastSnapshotPath"),
            Literal::new_simple_literal(snapshot_path).into(),
        )?;

        // Increment snapshotCount via a query + update
        let count_sparql = format!(
            "SELECT ?count WHERE {{ <{volume_iri_str}> <{PICLOUD_NS}snapshotCount> ?count }}"
        );
        let current_count = if let Ok(results) = self.execute_query(&count_sparql) {
            results.bindings.first()
                .and_then(|row| row.get("count"))
                .and_then(|v| v.get("value"))
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        } else {
            0
        };

        // Remove old count triple and insert new
        let old_count_str = current_count.to_string();
        let s = NamedNode::new(volume_iri_str)
            .map_err(|e| PiCloudError::Internal(format!("invalid IRI: {e}")))?;
        let p = NamedNode::new(format!("{PICLOUD_NS}snapshotCount"))
            .map_err(|e| PiCloudError::Internal(format!("invalid predicate: {e}")))?;
        let old_quad = Quad::new(
            s.clone(),
            p.clone(),
            Literal::new_simple_literal(&old_count_str),
            oxigraph::model::GraphName::DefaultGraph,
        );
        let _ = self.store.remove(&old_quad);

        self.insert_triple(
            volume_iri_str,
            &format!("{PICLOUD_NS}snapshotCount"),
            Literal::new_simple_literal((current_count + 1).to_string()).into(),
        )?;

        debug!(volume = volume_iri_str, "projected SnapshotCreated");
        Ok(())
    }

    /// Project a SnapshotDeleted event — mark snapshot as deleted in the graph.
    fn project_snapshot_deleted(&self, event: &EventEnvelope) -> Result<()> {
        let volume_iri_str = event.payload["volume_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let ts_fallback = event.timestamp.to_rfc3339();
        let deleted_at = event.payload["deleted_at"]
            .as_str()
            .unwrap_or(&ts_fallback);

        // Create a deletion event IRI
        let deletion_iri = format!("{}/snapshot-deletions/{}", volume_iri_str, deleted_at);
        self.insert_triple(
            &deletion_iri,
            RDF_TYPE,
            picloud_term("SnapshotDeleted").into(),
        )?;
        self.insert_triple(
            &deletion_iri,
            &format!("{PICLOUD_NS}volume"),
            NamedNode::new(volume_iri_str)
                .map_err(|e| PiCloudError::Internal(format!("invalid volume IRI: {e}")))?
                .into(),
        )?;
        self.insert_triple(
            &deletion_iri,
            &format!("{PICLOUD_NS}deletedAt"),
            Literal::new_simple_literal(deleted_at).into(),
        )?;

        debug!(volume = volume_iri_str, "projected SnapshotDeleted");
        Ok(())
    }

    /// Project a RestoreCompleted event — mark restore as completed in the graph.
    fn project_restore_completed(&self, event: &EventEnvelope) -> Result<()> {
        let volume_iri_str = event.payload["volume_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let ts_fallback = event.timestamp.to_rfc3339();
        let completed_at = event.payload["completed_at"]
            .as_str()
            .unwrap_or(&ts_fallback);

        let restore_iri = format!("{}/restores/{}", volume_iri_str, completed_at);
        self.insert_triple(
            &restore_iri,
            RDF_TYPE,
            picloud_term("RestoreCompleted").into(),
        )?;
        self.insert_triple(
            &restore_iri,
            &format!("{PICLOUD_NS}volume"),
            NamedNode::new(volume_iri_str)
                .map_err(|e| PiCloudError::Internal(format!("invalid volume IRI: {e}")))?
                .into(),
        )?;
        self.insert_triple(
            &restore_iri,
            &format!("{PICLOUD_NS}completedAt"),
            Literal::new_simple_literal(completed_at).into(),
        )?;

        debug!(volume = volume_iri_str, "projected RestoreCompleted");
        Ok(())
    }

    /// Project a BackupCompleted event — update volume backup status triples.
    fn project_backup_completed(&self, event: &EventEnvelope) -> Result<()> {
        let volume_iri_str = event.payload["volume_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let ts_fallback = event.timestamp.to_rfc3339();
        let completed_at = event.payload["completed_at"]
            .as_str()
            .unwrap_or(&ts_fallback);

        // Create a backup event IRI
        let backup_iri = format!("{}/backups/{}", volume_iri_str, completed_at);
        self.insert_triple(
            &backup_iri,
            RDF_TYPE,
            picloud_term("BackupCompleted").into(),
        )?;
        self.insert_triple(
            &backup_iri,
            &format!("{PICLOUD_NS}volume"),
            NamedNode::new(volume_iri_str)
                .map_err(|e| PiCloudError::Internal(format!("invalid volume IRI: {e}")))?
                .into(),
        )?;
        self.insert_triple(
            &backup_iri,
            &format!("{PICLOUD_NS}completedAt"),
            Literal::new_simple_literal(completed_at).into(),
        )?;

        // Update volume backup status
        self.insert_triple(
            volume_iri_str,
            &format!("{PICLOUD_NS}lastBackupAt"),
            Literal::new_simple_literal(completed_at).into(),
        )?;
        self.insert_triple(
            volume_iri_str,
            &format!("{PICLOUD_NS}lastBackupStatus"),
            Literal::new_simple_literal("success").into(),
        )?;

        debug!(volume = volume_iri_str, "projected BackupCompleted");
        Ok(())
    }

    /// Project a BackupFailed event — update volume backup status and create alert.
    fn project_backup_failed(&self, event: &EventEnvelope) -> Result<()> {
        let volume_iri_str = event.payload["volume_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let reason = event.payload["reason"]
            .as_str()
            .unwrap_or("unknown");

        // Update volume backup status
        self.insert_triple(
            volume_iri_str,
            &format!("{PICLOUD_NS}lastBackupStatus"),
            Literal::new_simple_literal("failed").into(),
        )?;

        // Create an alert for backup failure (ADR-047)
        let alert_iri = format!("{}/alerts/backupfailed", volume_iri_str);
        self.insert_triple(&alert_iri, RDF_TYPE, picloud_term("Alert").into())?;
        self.insert_triple(
            &alert_iri,
            &format!("{PICLOUD_NS}alertType"),
            Literal::new_simple_literal("BackupFailed").into(),
        )?;
        self.insert_triple(
            &alert_iri,
            &format!("{PICLOUD_NS}alertSeverity"),
            Literal::new_simple_literal("critical").into(),
        )?;
        self.insert_triple(
            &alert_iri,
            &format!("{PICLOUD_NS}alertMessage"),
            Literal::new_simple_literal(&format!("Offsite backup failed: {}", reason)).into(),
        )?;
        if let Ok(node) = NamedNode::new(volume_iri_str) {
            self.insert_triple(
                &alert_iri,
                &format!("{PICLOUD_NS}alertResource"),
                node.into(),
            )?;
        }

        debug!(volume = volume_iri_str, "projected BackupFailed");
        Ok(())
    }

    // ---- Alert Projection (ADR-041) ----

    fn project_alert_fired(&self, event: &EventEnvelope) -> Result<()> {
        let alert_type = event.payload["alert_type"].as_str().unwrap_or_default();
        let severity = event.payload["severity"].as_str().unwrap_or("warning");
        let resource_iri = event.payload["resource_iri"].as_str().unwrap_or(event.source.as_str());
        let rule_iri = event.payload["rule_iri"].as_str().unwrap_or_default();
        let message = event.payload["message"].as_str().unwrap_or_default();

        // Create a deterministic alert IRI from type + resource
        let alert_iri = format!("{}/alerts/{}", resource_iri, alert_type.to_lowercase());

        self.insert_triple(&alert_iri, RDF_TYPE, picloud_term("Alert").into())?;
        self.insert_triple(&alert_iri, &format!("{PICLOUD_NS}alertType"), Literal::new_simple_literal(alert_type).into())?;
        self.insert_triple(&alert_iri, &format!("{PICLOUD_NS}severity"), Literal::new_simple_literal(severity).into())?;
        self.insert_triple(&alert_iri, &format!("{PICLOUD_NS}message"), Literal::new_simple_literal(message).into())?;
        if let Ok(node) = NamedNode::new(resource_iri) {
            self.insert_triple(&alert_iri, &format!("{PICLOUD_NS}alertResource"), node.into())?;
        }
        if !rule_iri.is_empty() {
            if let Ok(node) = NamedNode::new(rule_iri) {
                self.insert_triple(&alert_iri, &format!("{PICLOUD_NS}alertRule"), node.into())?;
            }
        }

        debug!(alert_type = alert_type, resource = resource_iri, "projected AlertFired");
        Ok(())
    }

    fn project_alert_resolved(&self, event: &EventEnvelope) -> Result<()> {
        let alert_type = event.payload["alert_type"].as_str().unwrap_or_default();
        let resource_iri = event.payload["resource_iri"].as_str().unwrap_or(event.source.as_str());

        let alert_iri = format!("{}/alerts/{}", resource_iri, alert_type.to_lowercase());
        self.remove_triples_about_all_graphs(&alert_iri)?;

        debug!(alert_type = alert_type, resource = resource_iri, "projected AlertResolved");
        Ok(())
    }

    // ---- Capability Projection (ADR-055) ----

    fn project_capability_declared(&self, event: &EventEnvelope) -> Result<()> {
        let name = event.payload["name"].as_str().unwrap_or_default();
        let version = event.payload["version"].as_str().unwrap_or_default();
        let input_event = event.payload["input_event"].as_str().unwrap_or_default();
        let output_event = event.payload["output_event"].as_str().unwrap_or_default();
        let capability_iri = event.payload["capability_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());

        self.insert_triple(capability_iri, RDF_TYPE, picloud_term("Capability").into())?;
        self.insert_triple(capability_iri, &format!("{PICLOUD_NS}name"), Literal::new_simple_literal(name).into())?;
        self.insert_triple(capability_iri, &format!("{PICLOUD_NS}version"), Literal::new_simple_literal(version).into())?;
        self.insert_triple(capability_iri, &format!("{PICLOUD_NS}inputEvent"), Literal::new_simple_literal(input_event).into())?;
        self.insert_triple(capability_iri, &format!("{PICLOUD_NS}outputEvent"), Literal::new_simple_literal(output_event).into())?;
        self.insert_triple(capability_iri, &format!("{PICLOUD_NS}status"), Literal::new_simple_literal("declared").into())?;

        debug!(capability = name, "projected CapabilityDeclared");
        Ok(())
    }

    fn project_capability_ready(&self, event: &EventEnvelope) -> Result<()> {
        let capability_iri = event.payload["capability_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        self.update_status(capability_iri, "ready", None)?;
        debug!(capability_iri = capability_iri, "projected CapabilityReady");
        Ok(())
    }

    fn project_capability_implementor_added(&self, event: &EventEnvelope) -> Result<()> {
        let capability_iri = event.payload["capability_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let product_iri = event.payload["product_iri"].as_str().unwrap_or_default();
        let product_name = event.payload["product_name"].as_str().unwrap_or_default();

        self.insert_triple(
            capability_iri,
            &format!("{PICLOUD_NS}implementedBy"),
            NamedNode::new(product_iri)
                .map_err(|e| PiCloudError::Internal(format!("invalid product IRI: {e}")))?
                .into(),
        )?;

        debug!(capability_iri = capability_iri, product = product_name, "projected CapabilityImplementorAdded");
        Ok(())
    }

    fn project_capability_implementor_removed(&self, event: &EventEnvelope) -> Result<()> {
        let capability_iri = event.payload["capability_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let product_iri = event.payload["product_iri"].as_str().unwrap_or_default();

        // Remove the implementedBy triple
        if let (Ok(subj), Ok(pred), Ok(obj)) = (
            NamedNode::new(capability_iri),
            NamedNode::new(format!("{PICLOUD_NS}implementedBy")),
            NamedNode::new(product_iri),
        ) {
            let _ = self.store.remove(&Quad::new(subj, pred, obj, GraphName::DefaultGraph));
        }

        debug!(capability_iri = capability_iri, "projected CapabilityImplementorRemoved");
        Ok(())
    }

    fn project_capability_consumer_added(&self, event: &EventEnvelope) -> Result<()> {
        let capability_iri = event.payload["capability_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let product_iri = event.payload["product_iri"].as_str().unwrap_or_default();
        let product_name = event.payload["product_name"].as_str().unwrap_or_default();

        self.insert_triple(
            capability_iri,
            &format!("{PICLOUD_NS}consumedBy"),
            NamedNode::new(product_iri)
                .map_err(|e| PiCloudError::Internal(format!("invalid product IRI: {e}")))?
                .into(),
        )?;

        debug!(capability_iri = capability_iri, product = product_name, "projected CapabilityConsumerAdded");
        Ok(())
    }

    fn project_capability_deleted(&self, event: &EventEnvelope) -> Result<()> {
        let capability_iri = event.payload["capability_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        self.remove_triples_about_all_graphs(capability_iri)?;
        debug!(capability_iri = capability_iri, "projected CapabilityDeleted");
        Ok(())
    }

    // ---- Data Domain / Data Product Projection (ADR-056) ----

    fn project_data_domain_declared(&self, event: &EventEnvelope) -> Result<()> {
        let name = event.payload["name"].as_str().unwrap_or_default();
        let steward = event.payload["steward"].as_str().unwrap_or_default();
        let sensitivity = event.payload["sensitivity"].as_str().unwrap_or("internal");
        let domain_iri = event.payload["domain_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());

        self.insert_triple(domain_iri, RDF_TYPE, picloud_term("DataDomain").into())?;
        self.insert_triple(domain_iri, &format!("{PICLOUD_NS}name"), Literal::new_simple_literal(name).into())?;
        self.insert_triple(domain_iri, &format!("{PICLOUD_NS}steward"), Literal::new_simple_literal(steward).into())?;
        self.insert_triple(domain_iri, &format!("{PICLOUD_NS}sensitivity"), Literal::new_simple_literal(sensitivity).into())?;
        self.insert_triple(domain_iri, &format!("{PICLOUD_NS}status"), Literal::new_simple_literal("declared").into())?;

        // Optional human-readable description (ADR-056, FT-065).
        if let Some(description) = event.payload["description"].as_str() {
            if !description.is_empty() {
                self.insert_triple(
                    domain_iri,
                    &format!("{PICLOUD_NS}description"),
                    Literal::new_simple_literal(description).into(),
                )?;
            }
        }

        debug!(domain = name, "projected DataDomainDeclared");
        Ok(())
    }

    fn project_data_domain_updated(&self, event: &EventEnvelope) -> Result<()> {
        let domain_iri = event.payload["domain_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let steward = event.payload["steward"].as_str().unwrap_or_default();
        let sensitivity = event.payload["sensitivity"].as_str().unwrap_or("internal");

        // Replace mutable metadata fields — remove old values, insert new ones.
        self.remove_triple(domain_iri, &format!("{PICLOUD_NS}steward"))?;
        self.insert_triple(
            domain_iri,
            &format!("{PICLOUD_NS}steward"),
            Literal::new_simple_literal(steward).into(),
        )?;

        self.remove_triple(domain_iri, &format!("{PICLOUD_NS}sensitivity"))?;
        self.insert_triple(
            domain_iri,
            &format!("{PICLOUD_NS}sensitivity"),
            Literal::new_simple_literal(sensitivity).into(),
        )?;

        // Replace description only when a new value is provided.
        if let Some(description) = event.payload["description"].as_str() {
            self.remove_triple(domain_iri, &format!("{PICLOUD_NS}description"))?;
            if !description.is_empty() {
                self.insert_triple(
                    domain_iri,
                    &format!("{PICLOUD_NS}description"),
                    Literal::new_simple_literal(description).into(),
                )?;
            }
        }

        debug!(domain_iri = domain_iri, steward = steward, sensitivity = sensitivity, "projected DataDomainUpdated");
        Ok(())
    }

    fn project_data_domain_deleted(&self, event: &EventEnvelope) -> Result<()> {
        let domain_iri = event.payload["domain_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        self.remove_triples_about_all_graphs(domain_iri)?;
        debug!(domain_iri = domain_iri, "projected DataDomainDeleted");
        Ok(())
    }

    fn project_data_product_declared(&self, event: &EventEnvelope) -> Result<()> {
        let name = event.payload["name"].as_str().unwrap_or_default();
        let product = event.payload["product"].as_str().unwrap_or_default();
        let domain = event.payload["domain"].as_str().unwrap_or_default();
        let version = event.payload["version"].as_str().unwrap_or_default();
        let dp_iri = event.payload["data_product_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());

        self.insert_triple(dp_iri, RDF_TYPE, picloud_term("DataProduct").into())?;
        self.insert_triple(dp_iri, &format!("{PICLOUD_NS}name"), Literal::new_simple_literal(name).into())?;
        self.insert_triple(dp_iri, &format!("{PICLOUD_NS}product"), Literal::new_simple_literal(product).into())?;
        self.insert_triple(dp_iri, &format!("{PICLOUD_NS}domain"), Literal::new_simple_literal(domain).into())?;
        self.insert_triple(dp_iri, &format!("{PICLOUD_NS}version"), Literal::new_simple_literal(version).into())?;
        self.insert_triple(dp_iri, &format!("{PICLOUD_NS}status"), Literal::new_simple_literal("declared").into())?;

        // Link to the data domain
        let cluster_root = self.iri_builder.cluster_root();
        let domain_iri = format!("{}/data-domains/{domain}", cluster_root.as_str().trim_end_matches('/'));
        if let Ok(domain_node) = NamedNode::new(&domain_iri) {
            self.insert_triple(dp_iri, &format!("{PICLOUD_NS}belongsToDomain"), domain_node.into())?;
        }

        // Link to the owning product
        let product_iri = self.iri_builder.product(product);
        if let Ok(product_node) = NamedNode::new(product_iri.as_str()) {
            self.insert_triple(dp_iri, &format!("{PICLOUD_NS}producedBy"), product_node.into())?;
        }

        // Store freshness SLO (maxAge) so the SLO monitor can detect breaches (FT-068)
        if let Some(max_age) = event.payload["max_age"].as_str() {
            self.insert_triple(
                dp_iri,
                &format!("{PICLOUD_NS}maxAge"),
                Literal::new_simple_literal(max_age).into(),
            )?;
        }

        debug!(data_product = name, product = product, "projected DataProductDeclared");
        Ok(())
    }

    fn project_data_product_updated(&self, event: &EventEnvelope) -> Result<()> {
        let dp_iri = event.payload["data_product_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let version = event.payload["version"].as_str().unwrap_or_default();
        let domain = event.payload["domain"].as_str().unwrap_or_default();

        // Replace mutable metadata fields — remove old values, insert new ones.
        self.remove_triple(dp_iri, &format!("{PICLOUD_NS}version"))?;
        self.insert_triple(
            dp_iri,
            &format!("{PICLOUD_NS}version"),
            Literal::new_simple_literal(version).into(),
        )?;

        self.remove_triple(dp_iri, &format!("{PICLOUD_NS}domain"))?;
        self.insert_triple(
            dp_iri,
            &format!("{PICLOUD_NS}domain"),
            Literal::new_simple_literal(domain).into(),
        )?;

        // Update domain link
        self.remove_triple(dp_iri, &format!("{PICLOUD_NS}belongsToDomain"))?;
        let cluster_root = self.iri_builder.cluster_root();
        let domain_iri = format!("{}/data-domains/{domain}", cluster_root.as_str().trim_end_matches('/'));
        if let Ok(domain_node) = NamedNode::new(&domain_iri) {
            self.insert_triple(dp_iri, &format!("{PICLOUD_NS}belongsToDomain"), domain_node.into())?;
        }

        // Update freshness SLO if provided
        self.remove_triple(dp_iri, &format!("{PICLOUD_NS}maxAge"))?;
        if let Some(max_age) = event.payload["max_age"].as_str() {
            self.insert_triple(
                dp_iri,
                &format!("{PICLOUD_NS}maxAge"),
                Literal::new_simple_literal(max_age).into(),
            )?;
        }

        debug!(data_product = dp_iri, version = version, "projected DataProductUpdated");
        Ok(())
    }

    fn project_data_product_refreshed(&self, event: &EventEnvelope) -> Result<()> {
        let dp_iri = event.payload["data_product_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        let triple_count = event.payload["triple_count"].as_u64().unwrap_or(0);
        let ts_fallback = event.timestamp.to_rfc3339();
        let ts = event.payload["refreshed_at"]
            .as_str()
            .unwrap_or(&ts_fallback);

        self.update_status(dp_iri, "ready", None)?;
        // Remove previous lastRefreshed / tripleCount so only the latest value remains (FT-068)
        self.remove_triple(dp_iri, &format!("{PICLOUD_NS}lastRefreshed"))?;
        self.remove_triple(dp_iri, &format!("{PICLOUD_NS}tripleCount"))?;
        self.insert_triple(
            dp_iri,
            &format!("{PICLOUD_NS}lastRefreshed"),
            Literal::new_typed_literal(
                ts.to_string(),
                NamedNode::new("http://www.w3.org/2001/XMLSchema#dateTime").expect("valid XSD IRI"),
            ).into(),
        )?;
        self.insert_triple(
            dp_iri,
            &format!("{PICLOUD_NS}tripleCount"),
            Literal::new_typed_literal(
                triple_count.to_string(),
                NamedNode::new("http://www.w3.org/2001/XMLSchema#unsignedLong").expect("valid XSD IRI"),
            ).into(),
        )?;

        debug!(data_product = dp_iri, triple_count = triple_count, "projected DataProductRefreshed");
        Ok(())
    }

    fn project_data_product_deleted(&self, event: &EventEnvelope) -> Result<()> {
        let dp_iri = event.payload["data_product_iri"]
            .as_str()
            .unwrap_or(event.source.as_str());
        self.remove_triples_about_all_graphs(dp_iri)?;
        debug!(data_product = dp_iri, "projected DataProductDeleted");
        Ok(())
    }

    // ---- RDFS Inference (ADR-039) ----

    /// Materialise RDFS subclass inference.
    ///
    /// Scans all `rdfs:subClassOf` triples in the store. For every instance
    /// typed as the subclass, asserts the superclass type as well. This is
    /// the simplest useful RDFS entailment rule (rdfs9):
    ///
    ///   ?x rdf:type ?sub . ?sub rdfs:subClassOf ?super => ?x rdf:type ?super
    ///
    /// Runs to a fixed point so transitive hierarchies are fully materialised.
    pub fn materialise_rdfs_subclass(&self) -> Result<usize> {
        let mut total_inferred = 0;
        loop {
            let mut inferred_this_pass = 0;

            // Collect all subClassOf declarations
            let _subclass_pred = NamedNode::new(RDFS_SUBCLASS_OF)
                .map_err(|e| PiCloudError::Internal(format!("invalid RDFS IRI: {e}")))?;
            let subclass_quads: Vec<Quad> = self
                .store
                .quads_for_pattern(
                    None,
                    Some(NamedNodeRef::new(RDFS_SUBCLASS_OF).unwrap()),
                    None,
                    None,
                )
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| PiCloudError::Internal(e.to_string()))?;

            let rdf_type_node = NamedNode::new(RDF_TYPE)
                .map_err(|e| PiCloudError::Internal(format!("invalid RDF_TYPE IRI: {e}")))?;

            for sc_quad in &subclass_quads {
                // sc_quad.subject = subclass, sc_quad.object = superclass
                let subclass = &sc_quad.subject;
                let superclass = match &sc_quad.object {
                    Term::NamedNode(n) => n,
                    _ => continue,
                };
                let graph = &sc_quad.graph_name;

                // Find all instances of the subclass
                let instances: Vec<Quad> = self
                    .store
                    .quads_for_pattern(
                        None,
                        Some(rdf_type_node.as_ref()),
                        Some(subclass.into()),
                        Some(graph.as_ref()),
                    )
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|e| PiCloudError::Internal(e.to_string()))?;

                for inst_quad in &instances {
                    // Check if superclass type already exists
                    let existing: Vec<Quad> = self
                        .store
                        .quads_for_pattern(
                            Some((&inst_quad.subject).into()),
                            Some(rdf_type_node.as_ref()),
                            Some(superclass.into()),
                            Some(graph.as_ref()),
                        )
                        .collect::<std::result::Result<Vec<_>, _>>()
                        .map_err(|e| PiCloudError::Internal(e.to_string()))?;

                    if existing.is_empty() {
                        self.store
                            .insert(QuadRef::new(
                                &inst_quad.subject,
                                &rdf_type_node,
                                superclass,
                                graph.as_ref(),
                            ))
                            .map_err(|e| PiCloudError::Internal(e.to_string()))?;
                        inferred_this_pass += 1;
                    }
                }
            }

            total_inferred += inferred_this_pass;
            if inferred_this_pass == 0 {
                break;
            }
        }

        if total_inferred > 0 {
            debug!(triples_inferred = total_inferred, "RDFS subclass materialisation complete");
        }
        Ok(total_inferred)
    }

    /// Materialise OWL transitive-property inference (OWL 2 RL rule prp-trp).
    ///
    /// Scans all properties declared as `owl:TransitiveProperty`. For each
    /// such property `p`, computes the transitive closure: if `(x, p, y)` and
    /// `(y, p, z)` both exist, asserts `(x, p, z)`.
    ///
    /// Runs to a fixed point so chains of arbitrary depth are fully materialised.
    pub fn materialise_owl_transitive_property(&self) -> Result<usize> {
        let mut total_inferred = 0;
        let rdf_type_node = NamedNode::new(RDF_TYPE)
            .map_err(|e| PiCloudError::Internal(format!("invalid RDF_TYPE IRI: {e}")))?;
        let owl_tp = NamedNode::new(OWL_TRANSITIVE_PROPERTY)
            .map_err(|e| PiCloudError::Internal(format!("invalid OWL IRI: {e}")))?;

        // Step 1: Collect all properties declared as owl:TransitiveProperty
        let tp_quads: Vec<Quad> = self
            .store
            .quads_for_pattern(
                None,
                Some(rdf_type_node.as_ref()),
                Some((&owl_tp).into()),
                None,
            )
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;

        let transitive_props: Vec<NamedNode> = tp_quads
            .iter()
            .filter_map(|q| match &q.subject {
                Subject::NamedNode(n) => Some(n.clone()),
                _ => None,
            })
            .collect();

        if transitive_props.is_empty() {
            return Ok(0);
        }

        // Step 2: For each transitive property, compute fixed-point closure
        for prop in &transitive_props {
            loop {
                let mut inferred_this_pass = 0;

                // Collect all (x, prop, y) triples across all graphs
                let all_triples: Vec<Quad> = self
                    .store
                    .quads_for_pattern(None, Some(prop.as_ref()), None, None)
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|e| PiCloudError::Internal(e.to_string()))?;

                for left in &all_triples {
                    // left is (x, prop, y)
                    let y = match &left.object {
                        Term::NamedNode(n) => n,
                        _ => continue,
                    };
                    let graph = &left.graph_name;

                    // Find all (y, prop, z) in the same graph
                    let rights: Vec<Quad> = self
                        .store
                        .quads_for_pattern(
                            Some(y.into()),
                            Some(prop.as_ref()),
                            None,
                            Some(graph.as_ref()),
                        )
                        .collect::<std::result::Result<Vec<_>, _>>()
                        .map_err(|e| PiCloudError::Internal(e.to_string()))?;

                    for right in &rights {
                        // right is (y, prop, z) — infer (x, prop, z)
                        let z = &right.object;

                        // Check if (x, prop, z) already exists in this graph
                        let existing: Vec<Quad> = self
                            .store
                            .quads_for_pattern(
                                Some((&left.subject).into()),
                                Some(prop.as_ref()),
                                Some(z.into()),
                                Some(graph.as_ref()),
                            )
                            .collect::<std::result::Result<Vec<_>, _>>()
                            .map_err(|e| PiCloudError::Internal(e.to_string()))?;

                        if existing.is_empty() {
                            self.store
                                .insert(QuadRef::new(
                                    &left.subject,
                                    prop,
                                    z,
                                    graph.as_ref(),
                                ))
                                .map_err(|e| PiCloudError::Internal(e.to_string()))?;
                            inferred_this_pass += 1;
                        }
                    }
                }

                total_inferred += inferred_this_pass;
                if inferred_this_pass == 0 {
                    break;
                }
            }
        }

        if total_inferred > 0 {
            debug!(
                triples_inferred = total_inferred,
                "OWL transitive-property materialisation complete"
            );
        }
        Ok(total_inferred)
    }

    /// Load ontology triples (Turtle format) and materialise RDFS/OWL inference.
    ///
    /// Accepts a Turtle string containing rdfs:subClassOf, owl:TransitiveProperty,
    /// and other ontology axioms. After loading, runs both
    /// `materialise_rdfs_subclass` and `materialise_owl_transitive_property`
    /// to derive inferred triples.
    pub fn load_ontology(&self, turtle: &str, graph: Option<&str>) -> Result<usize> {
        use oxigraph::io::RdfParser;

        let graph_name = match graph {
            Some(g) => {
                let nn = NamedNode::new(g)
                    .map_err(|e| PiCloudError::Internal(format!("invalid graph IRI: {e}")))?;
                GraphName::NamedNode(nn)
            }
            None => GraphName::DefaultGraph,
        };

        let parser = RdfParser::from_format(oxigraph::io::RdfFormat::Turtle)
            .with_base_iri(PICLOUD_NS)
            .map_err(|e| PiCloudError::Internal(format!("invalid base IRI: {e}")))?
            .with_default_graph(graph_name);

        // Use the non-deprecated load_from_reader API
        self.store
            .load_from_reader(parser, turtle.as_bytes())
            .map_err(|e| PiCloudError::Internal(format!("ontology load failed: {e}")))?;

        // Materialise RDFS subclass hierarchy
        let rdfs_inferred = self.materialise_rdfs_subclass()?;

        // Materialise OWL transitive-property closure (ADR-039)
        let owl_inferred = self.materialise_owl_transitive_property()?;

        let total_inferred = rdfs_inferred + owl_inferred;
        info!(
            rdfs_inferred,
            owl_inferred,
            total_inferred,
            "Ontology loaded and RDFS/OWL materialised"
        );
        Ok(total_inferred)
    }

    // ---- Shadow Graph Replay (ADR-035) ----

    /// Project a single event into a specific named graph (shadow graph).
    /// Used during replay to build the shadow projection without
    /// affecting the live default graph.
    fn project_to_shadow(&self, event: &EventEnvelope, shadow_graph: &str) -> Result<()> {
        // For replay, we re-use the same projection logic but target the
        // shadow graph. We temporarily project into the default graph and
        // then we use a simpler approach: insert key triples into the shadow.
        //
        // The simplest correct approach: project normally (which updates
        // default graph), then we will swap. But for shadow isolation, we
        // insert directly into the shadow graph using the same extraction
        // logic.
        let resource_iri = event.payload.get("resource_iri")
            .or_else(|| event.payload.get("node_iri"))
            .or_else(|| event.payload.get("identity_iri"))
            .or_else(|| event.payload.get("product_iri"))
            .or_else(|| event.payload.get("config_iri"))
            .or_else(|| event.payload.get("flag_iri"))
            .or_else(|| event.payload.get("group_iri"))
            .and_then(|v| v.as_str())
            .unwrap_or(event.source.as_str());

        // Record that this event was projected into the shadow graph
        self.insert_triple_in_graph(
            resource_iri,
            RDF_TYPE,
            picloud_term("ReplayedResource").into(),
            shadow_graph,
        )?;

        // Store the event type for audit
        self.insert_triple_in_graph(
            resource_iri,
            &format!("{PICLOUD_NS}lastEventType"),
            Literal::new_simple_literal(&event.event_type).into(),
            shadow_graph,
        )?;

        Ok(())
    }

    /// Execute a replay: project filtered events into a shadow graph,
    /// then atomically swap the shadow graph with the target live graph.
    ///
    /// Returns the number of events replayed.
    pub fn execute_replay(
        &self,
        replay_id: Uuid,
        events: &[EventEnvelope],
        target_graph: Option<&str>,
    ) -> Result<ReplayResult> {
        let shadow_graph_iri = self.iri_builder.replay_graph(&replay_id.to_string());
        let shadow_graph = shadow_graph_iri.as_str();

        // Ensure shadow graph exists
        self.store
            .insert_named_graph(
                NamedNodeRef::new(shadow_graph)
                    .map_err(|e| PiCloudError::Internal(format!("invalid shadow graph IRI: {e}")))?,
            )
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;

        // Project all events into the shadow graph
        let mut events_replayed = 0;
        for event in events {
            self.project_to_shadow(event, shadow_graph)?;
            events_replayed += 1;
        }

        // Atomic swap: if a target graph is specified, clear it and copy
        // shadow triples into it. If no target (platform-level), the
        // shadow graph stands on its own as the replayed projection.
        if let Some(target) = target_graph {
            let target_nn = NamedNode::new(target)
                .map_err(|e| PiCloudError::Internal(format!("invalid target graph IRI: {e}")))?;

            // Clear target graph
            let target_quads: Vec<Quad> = self
                .store
                .quads_for_pattern(None, None, None, Some(GraphNameRef::from(&target_nn)))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| PiCloudError::Internal(e.to_string()))?;
            for quad in &target_quads {
                self.store
                    .remove(quad)
                    .map_err(|e| PiCloudError::Internal(e.to_string()))?;
            }

            // Copy shadow triples into target
            let shadow_nn = NamedNode::new(shadow_graph)
                .map_err(|e| PiCloudError::Internal(format!("invalid shadow graph IRI: {e}")))?;
            let shadow_quads: Vec<Quad> = self
                .store
                .quads_for_pattern(None, None, None, Some(GraphNameRef::from(&shadow_nn)))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| PiCloudError::Internal(e.to_string()))?;
            for quad in &shadow_quads {
                let new_quad = QuadRef::new(
                    &quad.subject,
                    &quad.predicate,
                    &quad.object,
                    &target_nn,
                );
                self.store
                    .insert(new_quad)
                    .map_err(|e| PiCloudError::Internal(e.to_string()))?;
            }

            // Clean up shadow graph
            for quad in &shadow_quads {
                self.store
                    .remove(quad)
                    .map_err(|e| PiCloudError::Internal(e.to_string()))?;
            }

            info!(
                replay_id = %replay_id,
                events = events_replayed,
                target_graph = target,
                "Shadow graph swapped with target graph"
            );
        } else {
            info!(
                replay_id = %replay_id,
                events = events_replayed,
                shadow_graph = shadow_graph,
                "Shadow graph replay complete (no swap — platform level)"
            );
        }

        Ok(ReplayResult {
            replay_id,
            events_replayed,
            shadow_graph_iri: shadow_graph_iri.to_string(),
        })
    }

    /// Execute an aggregate-scoped replay (FT-082, ADR-035).
    ///
    /// Filters the provided events to include only those matching the
    /// specified aggregate type and/or aggregate IDs, then projects
    /// the matching events into a shadow graph and atomically swaps
    /// with the target graph.
    ///
    /// Supports:
    ///   - Single aggregate replay: one aggregate_id in the request
    ///   - Batch aggregate replay: up to 1000 aggregate_ids
    ///
    /// Events match an aggregate scope when:
    ///   1. Their payload contains `aggregate_type` matching the request, AND
    ///   2. Their payload contains `aggregate_id` matching one of the request IDs
    ///
    /// If `aggregate_ids` is empty, all events matching `aggregate_type` are included.
    /// If `aggregate_type` is None, only `aggregate_ids` are matched (via source IRI suffix).
    pub fn execute_aggregate_replay(
        &self,
        replay_id: Uuid,
        events: &[EventEnvelope],
        request: &ReplayRequest,
        target_graph: Option<&str>,
    ) -> Result<ReplayResult> {
        // Validate batch size (ADR-035: up to 1000 aggregates)
        if request.aggregate_ids.len() > 1000 {
            return Err(PiCloudError::ResourceValidationFailed {
                reason: format!(
                    "aggregate replay batch size {} exceeds maximum of 1000",
                    request.aggregate_ids.len()
                ),
            });
        }

        // Filter events by aggregate scope
        let filtered: Vec<&EventEnvelope> = events
            .iter()
            .filter(|event| {
                // Check time range
                if event.timestamp < request.from {
                    return false;
                }
                if let Some(ref to) = request.to {
                    if event.timestamp > *to {
                        return false;
                    }
                }

                // Check product scope
                if let Some(ref product) = request.product {
                    if event.product.as_deref() != Some(product.as_str()) {
                        return false;
                    }
                }

                // Check aggregate type filter
                let event_agg_type = event.payload.get("aggregate_type")
                    .and_then(|v| v.as_str());

                if let Some(ref agg_type) = request.aggregate_type {
                    match event_agg_type {
                        Some(t) if t == agg_type.as_str() => {}
                        _ => return false,
                    }
                }

                // Check aggregate ID filter
                if !request.aggregate_ids.is_empty() {
                    let event_agg_id = event.payload.get("aggregate_id")
                        .and_then(|v| v.as_str());
                    match event_agg_id {
                        Some(id) => {
                            if !request.aggregate_ids.iter().any(|a| a == id) {
                                return false;
                            }
                        }
                        None => return false,
                    }
                }

                true
            })
            .collect();

        // Build owned copies for execute_replay
        let filtered_owned: Vec<EventEnvelope> = filtered.into_iter().cloned().collect();

        self.execute_replay(replay_id, &filtered_owned, target_graph)
    }

    /// Capitalize a status string for use as a named node, e.g. "ready" -> "Ready".
    fn capitalize_status(status: &str) -> String {
        let mut chars = status.chars();
        match chars.next() {
            None => String::new(),
            Some(c) => c.to_uppercase().to_string() + chars.as_str(),
        }
    }

    /// Replace the status for a resource in the default graph
    /// (and optionally a product graph).
    ///
    /// Status is stored both as a named node (`picloud:Ready`) for SPARQL
    /// pattern matching and as a literal (`"ready"`) for backward compatibility.
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

        // Remove old status triples in default graph (both literal and named node forms).
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

        // Insert new status as named node (picloud:Ready, picloud:Declared, etc.)
        let status_capitalized = Self::capitalize_status(new_status);
        self.insert_triple(
            resource_iri,
            &status_pred,
            picloud_term(&status_capitalized).into(),
        )?;

        // Also insert as literal for backward compatibility with queries using string matching
        self.insert_triple(
            resource_iri,
            &format!("{PICLOUD_NS}statusLabel"),
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
                picloud_term(&status_capitalized).into(),
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
            "LeaderElected" => self.project_leader_elected(event),
            "ResourceDeclared" => self.project_resource_declared(event),
            "ResourceProvisioning" => self.project_resource_provisioning(event),
            "ResourceReady" => self.project_resource_ready(event),
            "ResourceFailed" => self.project_resource_failed(event),
            "ResourceDeleted" => self.project_resource_deleted(event),
            "IdentityCreated" => self.project_identity_created(event),
            "ProductDeployed" => self.project_product_deployed(event),
            "ProductDeleted" => self.project_product_deleted(event),
            "ProductUpgradeStarted" => self.project_product_upgrade_started(event),
            "ProductUpgradeCompleted" => self.project_product_upgrade_completed(event),
            "ProductUpgradeAborted" => self.project_product_upgrade_aborted(event),
            "MetricRecorded" => self.project_metric_recorded(event),
            "TagAdded" => self.project_tag_added(event),
            "TagRemoved" => self.project_tag_removed(event),
            "ConfigChanged" => self.project_config_changed(event),
            "FeatureFlagChanged" => self.project_feature_flag_changed(event),
            "GroupMembershipChanged" => self.project_group_membership_changed(event),
            "AlertFired" => self.project_alert_fired(event),
            "AlertResolved" => self.project_alert_resolved(event),
            // Replay lifecycle events are projected for audit (ADR-035)
            "ReplayStarted" | "ReplayProgress" | "ReplayCompleted" | "ReplayFailed" => {
                debug!(event_type = %event.event_type, "replay lifecycle event — recorded");
                Ok(())
            }
            // Snapshot & backup events (ADR-047) — project status triples
            "SnapshotCreated" => self.project_snapshot_created(event),
            "SnapshotDeleted" => self.project_snapshot_deleted(event),
            "SnapshotFailed" => {
                debug!(event_type = %event.event_type, "snapshot lifecycle event — recorded");
                Ok(())
            }
            "RestoreCompleted" => self.project_restore_completed(event),
            "RestoreRequested" | "RestoreFailed" => {
                debug!(event_type = %event.event_type, "restore lifecycle event — recorded");
                Ok(())
            }
            "BackupCompleted" => self.project_backup_completed(event),
            "BackupFailed" => self.project_backup_failed(event),
            "BackupStarted" => {
                debug!(event_type = %event.event_type, "backup lifecycle event — recorded");
                Ok(())
            }
            // OCI Registry events (ADR-054)
            "ImagePushed" => self.project_image_pushed(event),
            "ImageDeleted" => self.project_image_deleted(event),
            "ImageTagUpdated" => self.project_image_tag_updated(event),
            "RegistryGCStarted" => self.project_registry_gc_started(event),
            "RegistryGCCompleted" => self.project_registry_gc_completed(event),
            "RegistryAuthFailed" => {
                debug!("RegistryAuthFailed event — no RDF projection");
                Ok(())
            }
            // Capability events (ADR-055, FT-062)
            "CapabilityDeclared" => self.project_capability_declared(event),
            "CapabilityReady" => self.project_capability_ready(event),
            "CapabilityImplementorAdded" => self.project_capability_implementor_added(event),
            "CapabilityImplementorRemoved" => self.project_capability_implementor_removed(event),
            "CapabilityConsumerAdded" => self.project_capability_consumer_added(event),
            "CapabilityDeleted" => self.project_capability_deleted(event),
            "CapabilityUnfulfilled" | "CapabilityRoutingFailed" => {
                debug!(event_type = %event.event_type, "capability lifecycle event — recorded");
                Ok(())
            }
            // Ontology events (ADR-023, FT-053)
            "OntologyLoaded" => self.project_ontology_loaded(event),
            // Data Domain / Data Product events (ADR-056)
            "DataDomainDeclared" => self.project_data_domain_declared(event),
            "DataDomainUpdated" => self.project_data_domain_updated(event),
            "DataDomainDeleted" => self.project_data_domain_deleted(event),
            "DataProductDeclared" => self.project_data_product_declared(event),
            "DataProductUpdated" => self.project_data_product_updated(event),
            "DataProductRefreshed" => self.project_data_product_refreshed(event),
            "DataProductDeleted" => self.project_data_product_deleted(event),
            "DataProductReady" | "DataProductSLOBreached" | "DataProductSLORestored" => {
                debug!(event_type = %event.event_type, "data product lifecycle event — recorded");
                Ok(())
            }
            // Product event store events (ADR-032, FT-078)
            "ProductEventAppended" => self.project_product_event_appended(event),
            // Telemetry events (ADR-045) — no RDF projection needed
            "TelemetryAggregated" => {
                debug!("TelemetryAggregated event — no RDF projection");
                Ok(())
            }
            // Node drain events (FT-011)
            "NodeCordoned" => self.project_node_cordoned(event),
            "NodeUncordoned" => self.project_node_uncordoned(event),
            "NodeDrainStarted" => self.project_node_drain_started(event),
            "NodeDrainCompleted" => self.project_node_drain_completed(event),
            "NodeDrainFailed" => {
                debug!(event_type = %event.event_type, "drain lifecycle event — recorded");
                Ok(())
            }
            "WorkloadMigrated" => self.project_workload_migrated(event),
            // Log compaction events (FT-011) — no RDF projection needed
            "LogCompactionCompleted" => {
                debug!("LogCompactionCompleted event — no RDF projection");
                Ok(())
            }
            // Self-monitoring events (FT-011)
            "SelfMonitoringCheckCompleted" => self.project_self_monitoring(event),
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

    async fn query_turtle(&self, sparql: &str) -> Result<String> {
        self.execute_query_to_turtle(sparql)
    }
}

#[async_trait]
impl ReplayEngine for OxigraphProjector {
    async fn start_replay(&self, request: ReplayRequest) -> Result<Uuid> {
        let replay_id = Uuid::new_v4();

        // Determine the target graph for the swap
        let _target_graph = request.product.as_ref().map(|p| {
            self.iri_builder.product_graph(p).to_string()
        });

        // Filter events by time range — in a real implementation this
        // would query the event log. Here we create the shadow graph
        // structure so the HTTP layer can feed events through.
        let shadow_graph_iri = self.iri_builder.replay_graph(&replay_id.to_string());

        // Ensure shadow graph exists
        self.store
            .insert_named_graph(
                NamedNodeRef::new(shadow_graph_iri.as_str())
                    .map_err(|e| PiCloudError::Internal(format!("invalid shadow graph IRI: {e}")))?,
            )
            .map_err(|e| PiCloudError::Internal(e.to_string()))?;

        // Record replay metadata in the shadow graph
        let replay_meta_iri = format!("{}/meta", shadow_graph_iri.as_str());
        self.insert_triple_in_graph(
            &replay_meta_iri,
            RDF_TYPE,
            picloud_term("ReplayOperation").into(),
            shadow_graph_iri.as_str(),
        )?;
        self.insert_triple_in_graph(
            &replay_meta_iri,
            &format!("{PICLOUD_NS}replayId"),
            Literal::new_simple_literal(replay_id.to_string()).into(),
            shadow_graph_iri.as_str(),
        )?;
        self.insert_triple_in_graph(
            &replay_meta_iri,
            &format!("{PICLOUD_NS}replayFrom"),
            Literal::new_simple_literal(request.from.to_rfc3339()).into(),
            shadow_graph_iri.as_str(),
        )?;
        if let Some(ref to) = request.to {
            self.insert_triple_in_graph(
                &replay_meta_iri,
                &format!("{PICLOUD_NS}replayTo"),
                Literal::new_simple_literal(to.to_rfc3339()).into(),
                shadow_graph_iri.as_str(),
            )?;
        }
        if let Some(ref product) = request.product {
            self.insert_triple_in_graph(
                &replay_meta_iri,
                &format!("{PICLOUD_NS}replayProduct"),
                Literal::new_simple_literal(product).into(),
                shadow_graph_iri.as_str(),
            )?;
        }

        // Record aggregate scope metadata if provided (FT-082)
        if let Some(ref aggregate_type) = request.aggregate_type {
            self.insert_triple_in_graph(
                &replay_meta_iri,
                &format!("{PICLOUD_NS}replayAggregateType"),
                Literal::new_simple_literal(aggregate_type).into(),
                shadow_graph_iri.as_str(),
            )?;
        }
        for aggregate_id in &request.aggregate_ids {
            self.insert_triple_in_graph(
                &replay_meta_iri,
                &format!("{PICLOUD_NS}replayAggregateId"),
                Literal::new_simple_literal(aggregate_id).into(),
                shadow_graph_iri.as_str(),
            )?;
        }

        info!(
            replay_id = %replay_id,
            product = ?request.product,
            from = %request.from,
            to = ?request.to,
            aggregate_type = ?request.aggregate_type,
            aggregate_ids_count = request.aggregate_ids.len(),
            "Replay operation started — shadow graph created"
        );

        Ok(replay_id)
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

        // Verify declared status (now stored as named node picloud:Declared).
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
            &format!("{}Declared", PICLOUD_NS)
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

        // Verify updated status (now stored as named node picloud:Ready).
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
            &format!("{}Ready", PICLOUD_NS)
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
            &format!("{}Failed", PICLOUD_NS)
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

    #[tokio::test]
    async fn test_project_product_deleted_removes_product_and_children() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();
        let product_iri = iri_builder.product("photo-app");

        // Deploy a product
        let deploy_event = EventEnvelope::new(
            iri_builder.event_schema("ProductDeployed", 1),
            "ProductDeployed",
            product_iri.clone(),
            Some("photo-app".to_string()),
            Uuid::new_v4(),
            serde_json::json!({
                "product_iri": product_iri.as_str(),
                "product_name": "photo-app",
                "version": "1.0.0",
            }),
        );
        projector.project(&deploy_event).await.unwrap();

        // Declare a child resource
        let container_iri = iri_builder.resource("photo-app", "containers", "api-server");
        let declared_event = EventEnvelope::new(
            iri_builder.event_schema("ResourceDeclared", 1),
            "ResourceDeclared",
            container_iri.clone(),
            Some("photo-app".to_string()),
            Uuid::new_v4(),
            serde_json::json!({
                "resource_iri": container_iri.as_str(),
                "resource_type": "Container",
                "product": "photo-app",
            }),
        );
        projector.project(&declared_event).await.unwrap();

        // Verify product exists
        let result = projector
            .query(&format!(
                "SELECT ?s WHERE {{ <{}> <{}resourceType> ?s }}",
                product_iri.as_str(),
                PICLOUD_NS,
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);

        // Delete the product
        let delete_event = EventEnvelope::new(
            iri_builder.event_schema("ProductDeleted", 1),
            "ProductDeleted",
            product_iri.clone(),
            Some("photo-app".to_string()),
            Uuid::new_v4(),
            serde_json::json!({
                "product_iri": product_iri.as_str(),
            }),
        );
        projector.project(&delete_event).await.unwrap();

        // Verify product triples are gone
        let result = projector
            .query(&format!(
                "SELECT ?s WHERE {{ <{}> <{}resourceType> ?s }}",
                product_iri.as_str(),
                PICLOUD_NS,
            ))
            .await
            .unwrap();
        assert!(result.bindings.is_empty());

        // Verify child resource triples in named graph are gone
        let result = projector
            .query_product(
                &product_iri,
                &format!("?res <{}resourceType> ?rtype", PICLOUD_NS),
            )
            .await
            .unwrap();
        assert!(result.bindings.is_empty());
    }

    #[tokio::test]
    async fn test_project_metric_recorded() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();
        let node_iri = iri_builder.node("pi-node-01");

        // First, join the node so it exists in the graph
        let join_event = make_node_joined_event(&iri_builder);
        projector.project(&join_event).await.unwrap();

        // Emit a MetricRecorded event
        let metric_event = EventEnvelope::new(
            iri_builder.event_schema("MetricRecorded", 1),
            "MetricRecorded",
            node_iri.clone(),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "node_iri": node_iri.as_str(),
                "metrics": [
                    { "name": "cpu_usage_percent", "value": 42.3, "unit": "percent" },
                    { "name": "memory_used_mb", "value": 8192.0, "unit": "mb" },
                    { "name": "memory_total_mb", "value": 16384.0, "unit": "mb" },
                    { "name": "disk_used_gb", "value": 312.0, "unit": "gb" },
                    { "name": "disk_total_gb", "value": 1000.0, "unit": "gb" },
                    { "name": "cpu_temp_celsius", "value": 58.1, "unit": "celsius" },
                ],
            }),
        );
        projector.project(&metric_event).await.unwrap();

        // Query for CPU usage
        let result = projector
            .query(&format!(
                "SELECT ?cpu WHERE {{ <{}> <{}cpuUsagePercent> ?cpu }}",
                node_iri.as_str(),
                PICLOUD_NS,
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(result.bindings[0]["cpu"]["value"].as_str().unwrap(), "42.3");

        // Query for memory
        let result = projector
            .query(&format!(
                "SELECT ?mem WHERE {{ <{}> <{}memoryUsedMb> ?mem }}",
                node_iri.as_str(),
                PICLOUD_NS,
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(result.bindings[0]["mem"]["value"].as_str().unwrap(), "8192");

        // Query for temperature
        let result = projector
            .query(&format!(
                "SELECT ?temp WHERE {{ <{}> <{}cpuTempCelsius> ?temp }}",
                node_iri.as_str(),
                PICLOUD_NS,
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(result.bindings[0]["temp"]["value"].as_str().unwrap(), "58.1");

        // Query for metricsUpdatedAt (should exist)
        let result = projector
            .query(&format!(
                "SELECT ?ts WHERE {{ <{}> <{}metricsUpdatedAt> ?ts }}",
                node_iri.as_str(),
                PICLOUD_NS,
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
    }

    #[tokio::test]
    async fn test_metric_recorded_upserts_values() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();
        let node_iri = iri_builder.node("pi-node-02");

        // Emit first metric event
        let event1 = EventEnvelope::new(
            iri_builder.event_schema("MetricRecorded", 1),
            "MetricRecorded",
            node_iri.clone(),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "node_iri": node_iri.as_str(),
                "metrics": [
                    { "name": "cpu_usage_percent", "value": 42.3, "unit": "percent" },
                ],
            }),
        );
        projector.project(&event1).await.unwrap();

        // Emit second metric event with updated value
        let event2 = EventEnvelope::new(
            iri_builder.event_schema("MetricRecorded", 1),
            "MetricRecorded",
            node_iri.clone(),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "node_iri": node_iri.as_str(),
                "metrics": [
                    { "name": "cpu_usage_percent", "value": 85.7, "unit": "percent" },
                ],
            }),
        );
        projector.project(&event2).await.unwrap();

        // Should have exactly one triple, with the updated value
        let result = projector
            .query(&format!(
                "SELECT ?cpu WHERE {{ <{}> <{}cpuUsagePercent> ?cpu }}",
                node_iri.as_str(),
                PICLOUD_NS,
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1, "should upsert, not append");
        assert_eq!(
            result.bindings[0]["cpu"]["value"].as_str().unwrap(),
            "85.7",
            "should reflect latest value"
        );

        // metricsUpdatedAt should also be exactly one triple
        let result = projector
            .query(&format!(
                "SELECT ?ts WHERE {{ <{}> <{}metricsUpdatedAt> ?ts }}",
                node_iri.as_str(),
                PICLOUD_NS,
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1, "timestamp should upsert");
    }

    #[tokio::test]
    async fn test_project_tag_added() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();
        let resource_iri = iri_builder.resource("photo-app", "containers", "api-server");

        let event = EventEnvelope::new(
            iri_builder.event_schema("TagAdded", 1),
            "TagAdded",
            resource_iri.clone(),
            Some("photo-app".to_string()),
            Uuid::new_v4(),
            serde_json::json!({
                "resource_iri": resource_iri.as_str(),
                "key": "team",
                "value": "backend",
            }),
        );

        projector.project(&event).await.unwrap();

        // Query for tags on the resource
        let result = projector
            .query(&format!(
                "SELECT ?key ?value WHERE {{ <{}> <{}tag> ?tag . ?tag <{}tagKey> ?key . ?tag <{}tagValue> ?value }}",
                resource_iri.as_str(), PICLOUD_NS, PICLOUD_NS, PICLOUD_NS
            ))
            .await
            .unwrap();

        assert_eq!(result.bindings.len(), 1);
        assert_eq!(result.bindings[0]["key"]["value"].as_str().unwrap(), "team");
        assert_eq!(result.bindings[0]["value"]["value"].as_str().unwrap(), "backend");
    }

    #[tokio::test]
    async fn test_project_tag_added_multiple() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();
        let resource_iri = iri_builder.resource("photo-app", "containers", "api-server");

        // Add two tags
        for (key, value) in [("team", "backend"), ("environment", "production")] {
            let event = EventEnvelope::new(
                iri_builder.event_schema("TagAdded", 1),
                "TagAdded",
                resource_iri.clone(),
                Some("photo-app".to_string()),
                Uuid::new_v4(),
                serde_json::json!({
                    "resource_iri": resource_iri.as_str(),
                    "key": key,
                    "value": value,
                }),
            );
            projector.project(&event).await.unwrap();
        }

        let result = projector
            .query(&format!(
                "SELECT ?key ?value WHERE {{ <{}> <{}tag> ?tag . ?tag <{}tagKey> ?key . ?tag <{}tagValue> ?value }} ORDER BY ?key",
                resource_iri.as_str(), PICLOUD_NS, PICLOUD_NS, PICLOUD_NS
            ))
            .await
            .unwrap();

        assert_eq!(result.bindings.len(), 2);
    }

    #[tokio::test]
    async fn test_project_tag_removed() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();
        let resource_iri = iri_builder.resource("photo-app", "containers", "api-server");

        // Add a tag
        let add_event = EventEnvelope::new(
            iri_builder.event_schema("TagAdded", 1),
            "TagAdded",
            resource_iri.clone(),
            Some("photo-app".to_string()),
            Uuid::new_v4(),
            serde_json::json!({
                "resource_iri": resource_iri.as_str(),
                "key": "team",
                "value": "backend",
            }),
        );
        projector.project(&add_event).await.unwrap();

        // Verify it was added
        let result = projector
            .query(&format!(
                "SELECT ?key WHERE {{ <{}> <{}tag> ?tag . ?tag <{}tagKey> ?key }}",
                resource_iri.as_str(), PICLOUD_NS, PICLOUD_NS
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);

        // Remove the tag
        let remove_event = EventEnvelope::new(
            iri_builder.event_schema("TagRemoved", 1),
            "TagRemoved",
            resource_iri.clone(),
            Some("photo-app".to_string()),
            Uuid::new_v4(),
            serde_json::json!({
                "resource_iri": resource_iri.as_str(),
                "key": "team",
                "value": "backend",
            }),
        );
        projector.project(&remove_event).await.unwrap();

        // Verify it was removed
        let result = projector
            .query(&format!(
                "SELECT ?key WHERE {{ <{}> <{}tag> ?tag . ?tag <{}tagKey> ?key }}",
                resource_iri.as_str(), PICLOUD_NS, PICLOUD_NS
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 0);
    }

    #[tokio::test]
    async fn test_find_resources_by_tag() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();

        // Tag two resources with "environment=production"
        let res1 = iri_builder.resource("photo-app", "containers", "api-server");
        let res2 = iri_builder.resource("photo-app", "containers", "worker");

        for resource_iri in [&res1, &res2] {
            let event = EventEnvelope::new(
                iri_builder.event_schema("TagAdded", 1),
                "TagAdded",
                resource_iri.clone(),
                Some("photo-app".to_string()),
                Uuid::new_v4(),
                serde_json::json!({
                    "resource_iri": resource_iri.as_str(),
                    "key": "environment",
                    "value": "production",
                }),
            );
            projector.project(&event).await.unwrap();
        }

        // Tag only res1 with "tier=api"
        let event = EventEnvelope::new(
            iri_builder.event_schema("TagAdded", 1),
            "TagAdded",
            res1.clone(),
            Some("photo-app".to_string()),
            Uuid::new_v4(),
            serde_json::json!({
                "resource_iri": res1.as_str(),
                "key": "tier",
                "value": "api",
            }),
        );
        projector.project(&event).await.unwrap();

        // Find all resources with environment=production
        let result = projector
            .query(&format!(
                "SELECT ?resource WHERE {{ ?resource <{}tag> ?tag . ?tag <{}tagKey> \"environment\" . ?tag <{}tagValue> \"production\" }}",
                PICLOUD_NS, PICLOUD_NS, PICLOUD_NS
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 2);

        // Find resources with tier=api
        let result = projector
            .query(&format!(
                "SELECT ?resource WHERE {{ ?resource <{}tag> ?tag . ?tag <{}tagKey> \"tier\" . ?tag <{}tagValue> \"api\" }}",
                PICLOUD_NS, PICLOUD_NS, PICLOUD_NS
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(
            result.bindings[0]["resource"]["value"].as_str().unwrap(),
            res1.as_str()
        );
    }

    // ---- RDFS Inference tests (ADR-039) ----

    #[tokio::test]
    async fn test_rdfs_subclass_materialisation() {
        let projector = OxigraphProjector::new().unwrap();

        // Declare ontology: ProductionContainer rdfs:subClassOf Container
        let ontology = r#"
            @prefix picloud: <https://picloud.local/ontology#> .
            @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

            picloud:ProductionContainer rdfs:subClassOf picloud:Container .
        "#;
        projector.load_ontology(ontology, None).unwrap();

        // Create an instance of ProductionContainer
        projector
            .insert_triple(
                "https://picloud.local/products/photo-app/containers/api",
                RDF_TYPE,
                picloud_term("ProductionContainer").into(),
            )
            .unwrap();

        // Run materialisation
        let inferred = projector.materialise_rdfs_subclass().unwrap();
        assert!(inferred >= 1, "should infer at least 1 superclass type triple");

        // Query for all Containers — should include the ProductionContainer instance
        let result = projector
            .query(&format!(
                "SELECT ?c WHERE {{ ?c <{RDF_TYPE}> <{PICLOUD_NS}Container> }}"
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(
            result.bindings[0]["c"]["value"].as_str().unwrap(),
            "https://picloud.local/products/photo-app/containers/api"
        );
    }

    #[tokio::test]
    async fn test_rdfs_transitive_subclass() {
        let projector = OxigraphProjector::new().unwrap();

        // A -> B -> C (transitive)
        let ontology = r#"
            @prefix picloud: <https://picloud.local/ontology#> .
            @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

            picloud:StagingContainer rdfs:subClassOf picloud:ProductionContainer .
            picloud:ProductionContainer rdfs:subClassOf picloud:Container .
        "#;
        projector.load_ontology(ontology, None).unwrap();

        // Create an instance of StagingContainer
        projector
            .insert_triple(
                "https://picloud.local/test/staging-1",
                RDF_TYPE,
                picloud_term("StagingContainer").into(),
            )
            .unwrap();

        projector.materialise_rdfs_subclass().unwrap();

        // Should be queryable as Container (two levels up)
        let result = projector
            .query(&format!(
                "SELECT ?c WHERE {{ ?c <{RDF_TYPE}> <{PICLOUD_NS}Container> }}"
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
    }

    #[tokio::test]
    async fn test_load_ontology_into_named_graph() {
        let projector = OxigraphProjector::new().unwrap();
        let graph = "https://picloud.local/products/photo-app/graph";

        let ontology = r#"
            @prefix picloud: <https://picloud.local/ontology#> .
            @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

            picloud:AdminRole rdfs:subClassOf picloud:OperatorRole .
        "#;
        projector.load_ontology(ontology, Some(graph)).unwrap();

        // Verify the subClassOf triple is in the named graph
        let result = projector
            .execute_query(&format!(
                "SELECT ?sub ?super WHERE {{ GRAPH <{graph}> {{ ?sub <{RDFS_SUBCLASS_OF}> ?super }} }}"
            ))
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
    }

    // ---- Replay tests (ADR-035) ----

    #[tokio::test]
    async fn test_shadow_graph_replay() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();

        // Create some events to replay
        let events: Vec<EventEnvelope> = (0..3)
            .map(|i| {
                let resource_iri = iri_builder.resource("photo-app", "containers", &format!("svc-{i}"));
                EventEnvelope::new(
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
                )
            })
            .collect();

        let replay_id = Uuid::new_v4();
        let result = projector
            .execute_replay(replay_id, &events, None)
            .unwrap();

        assert_eq!(result.events_replayed, 3);
        assert_eq!(result.replay_id, replay_id);

        // Verify shadow graph has the replayed resources
        let shadow_graph = projector.iri_builder().replay_graph(&replay_id.to_string());
        let query_result = projector
            .execute_query(&format!(
                "SELECT ?r WHERE {{ GRAPH <{}> {{ ?r <{RDF_TYPE}> <{PICLOUD_NS}ReplayedResource> }} }}",
                shadow_graph.as_str()
            ))
            .unwrap();
        assert_eq!(query_result.bindings.len(), 3);
    }

    #[tokio::test]
    async fn test_shadow_graph_swap_with_target() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();
        let target_graph = "https://picloud.local/products/photo-app/graph";

        // Put some existing data in the target graph
        projector
            .insert_triple_in_graph(
                "https://picloud.local/products/photo-app/containers/old",
                RDF_TYPE,
                picloud_term("Resource").into(),
                target_graph,
            )
            .unwrap();

        // Verify old data exists
        let result = projector
            .execute_query(&format!(
                "SELECT ?r WHERE {{ GRAPH <{target_graph}> {{ ?r <{RDF_TYPE}> ?t }} }}"
            ))
            .unwrap();
        assert_eq!(result.bindings.len(), 1);

        // Replay with swap
        let resource_iri = iri_builder.resource("photo-app", "containers", "new-svc");
        let events = vec![EventEnvelope::new(
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
        )];

        let replay_id = Uuid::new_v4();
        let result = projector
            .execute_replay(replay_id, &events, Some(target_graph))
            .unwrap();
        assert_eq!(result.events_replayed, 1);

        // Old data should be gone, new data should be present
        let result = projector
            .execute_query(&format!(
                "SELECT ?r WHERE {{ GRAPH <{target_graph}> {{ ?r <{RDF_TYPE}> <{PICLOUD_NS}ReplayedResource> }} }}"
            ))
            .unwrap();
        assert_eq!(result.bindings.len(), 1);

        // Shadow graph should be cleaned up
        let shadow_graph = projector.iri_builder().replay_graph(&replay_id.to_string());
        let result = projector
            .execute_query(&format!(
                "SELECT ?r WHERE {{ GRAPH <{}> {{ ?r ?p ?o }} }}",
                shadow_graph.as_str()
            ))
            .unwrap();
        assert!(result.bindings.is_empty());
    }

    #[tokio::test]
    async fn test_replay_engine_start() {
        use chrono::Utc;
        use picloud_domain::traits::ReplayEngine;

        let projector = OxigraphProjector::new().unwrap();
        let request = ReplayRequest {
            from: Utc::now() - chrono::Duration::hours(1),
            to: None,
            product: Some("photo-app".to_string()),
            aggregate_type: None,
            aggregate_ids: vec![],
        };

        let replay_id = projector.start_replay(request).await.unwrap();

        // Verify shadow graph was created with metadata
        let shadow_graph = projector.iri_builder().replay_graph(&replay_id.to_string());
        let result = projector
            .execute_query(&format!(
                "SELECT ?s WHERE {{ GRAPH <{}> {{ ?s <{RDF_TYPE}> <{PICLOUD_NS}ReplayOperation> }} }}",
                shadow_graph.as_str()
            ))
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
    }

    // ---- OCI Registry projection tests (ADR-054) ----

    #[tokio::test]
    async fn test_project_image_pushed() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();

        let event = EventEnvelope::new(
            iri_builder.event_schema("ImagePushed", 1),
            "ImagePushed",
            iri_builder.cluster_root(),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "repository": "myapp/api",
                "tag": "v1.0",
                "digest": "sha256:abc123",
                "size_bytes": 52428800_u64,
                "media_type": "application/vnd.oci.image.manifest.v1+json",
                "pushed_by": "admin",
            }),
        );

        projector.project(&event).await.unwrap();

        // Check repository was created
        let result = projector
            .query(&format!(
                "SELECT ?name WHERE {{ ?repo <{RDF_TYPE}> <{PICLOUD_NS}Repository> . ?repo <{PICLOUD_NS}repositoryName> ?name }}"
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(result.bindings[0]["name"]["value"].as_str().unwrap(), "myapp/api");

        // Check image was created with digest and size
        let result = projector
            .query(&format!(
                "SELECT ?digest ?size WHERE {{ ?img <{RDF_TYPE}> <{PICLOUD_NS}Image> . ?img <{PICLOUD_NS}digest> ?digest . ?img <{PICLOUD_NS}sizeBytes> ?size }}"
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(result.bindings[0]["digest"]["value"].as_str().unwrap(), "sha256:abc123");
        assert_eq!(result.bindings[0]["size"]["value"].as_str().unwrap(), "52428800");

        // Check tag was created and points to the image
        let result = projector
            .query(&format!(
                "SELECT ?tagName WHERE {{ ?tag <{RDF_TYPE}> <{PICLOUD_NS}ImageTag> . ?tag <{PICLOUD_NS}tagName> ?tagName }}"
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(result.bindings[0]["tagName"]["value"].as_str().unwrap(), "v1.0");

        // Check repo has image and tag links
        let result = projector
            .query(&format!(
                "SELECT ?img WHERE {{ ?repo <{PICLOUD_NS}hasImage> ?img }}"
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);

        let result = projector
            .query(&format!(
                "SELECT ?tag WHERE {{ ?repo <{PICLOUD_NS}hasTag> ?tag }}"
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
    }

    #[tokio::test]
    async fn test_project_image_pushed_without_tag() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();

        let event = EventEnvelope::new(
            iri_builder.event_schema("ImagePushed", 1),
            "ImagePushed",
            iri_builder.cluster_root(),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "repository": "myapp/api",
                "digest": "sha256:abc123",
                "size_bytes": 1024_u64,
                "media_type": "application/vnd.oci.image.manifest.v1+json",
            }),
        );

        projector.project(&event).await.unwrap();

        // Image should exist
        let result = projector
            .query(&format!(
                "SELECT ?img WHERE {{ ?img <{RDF_TYPE}> <{PICLOUD_NS}Image> }}"
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);

        // No tag should exist
        let result = projector
            .query(&format!(
                "SELECT ?tag WHERE {{ ?tag <{RDF_TYPE}> <{PICLOUD_NS}ImageTag> }}"
            ))
            .await
            .unwrap();
        assert!(result.bindings.is_empty());
    }

    #[tokio::test]
    async fn test_project_image_deleted() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();

        // First push an image
        let push_event = EventEnvelope::new(
            iri_builder.event_schema("ImagePushed", 1),
            "ImagePushed",
            iri_builder.cluster_root(),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "repository": "myapp/api",
                "tag": "v1.0",
                "digest": "sha256:abc123",
                "size_bytes": 1024_u64,
                "media_type": "application/vnd.oci.image.manifest.v1+json",
            }),
        );
        projector.project(&push_event).await.unwrap();

        // Verify image exists
        let result = projector
            .query(&format!(
                "SELECT ?img WHERE {{ ?img <{RDF_TYPE}> <{PICLOUD_NS}Image> }}"
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);

        // Now delete the image
        let delete_event = EventEnvelope::new(
            iri_builder.event_schema("ImageDeleted", 1),
            "ImageDeleted",
            iri_builder.cluster_root(),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "repository": "myapp/api",
                "digest": "sha256:abc123",
                "deleted_by": "admin",
            }),
        );
        projector.project(&delete_event).await.unwrap();

        // Image should be gone
        let result = projector
            .query(&format!(
                "SELECT ?img WHERE {{ ?img <{RDF_TYPE}> <{PICLOUD_NS}Image> }}"
            ))
            .await
            .unwrap();
        assert!(result.bindings.is_empty());

        // Tag pointing to deleted image should also be gone
        let result = projector
            .query(&format!(
                "SELECT ?tag WHERE {{ ?tag <{RDF_TYPE}> <{PICLOUD_NS}ImageTag> }}"
            ))
            .await
            .unwrap();
        assert!(result.bindings.is_empty());

        // Repository should still exist
        let result = projector
            .query(&format!(
                "SELECT ?repo WHERE {{ ?repo <{RDF_TYPE}> <{PICLOUD_NS}Repository> }}"
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
    }

    #[tokio::test]
    async fn test_project_image_tag_updated() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();

        // Push initial image with tag
        let push_event = EventEnvelope::new(
            iri_builder.event_schema("ImagePushed", 1),
            "ImagePushed",
            iri_builder.cluster_root(),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "repository": "myapp/api",
                "tag": "latest",
                "digest": "sha256:old111",
                "size_bytes": 1024_u64,
                "media_type": "application/vnd.oci.image.manifest.v1+json",
            }),
        );
        projector.project(&push_event).await.unwrap();

        // Push new image (so it exists in the store)
        let push_event2 = EventEnvelope::new(
            iri_builder.event_schema("ImagePushed", 1),
            "ImagePushed",
            iri_builder.cluster_root(),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "repository": "myapp/api",
                "digest": "sha256:new222",
                "size_bytes": 2048_u64,
                "media_type": "application/vnd.oci.image.manifest.v1+json",
            }),
        );
        projector.project(&push_event2).await.unwrap();

        // Update tag to point to new digest
        let update_event = EventEnvelope::new(
            iri_builder.event_schema("ImageTagUpdated", 1),
            "ImageTagUpdated",
            iri_builder.cluster_root(),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "repository": "myapp/api",
                "tag": "latest",
                "previous_digest": "sha256:old111",
                "new_digest": "sha256:new222",
            }),
        );
        projector.project(&update_event).await.unwrap();

        // Verify the tag now points to the new image
        let result = projector
            .query(&format!(
                "SELECT ?target WHERE {{ ?tag <{PICLOUD_NS}tagName> \"latest\" . ?tag <{PICLOUD_NS}pointsToDigest> ?target }}"
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        let target_iri = result.bindings[0]["target"]["value"].as_str().unwrap();
        assert!(target_iri.contains("sha256:new222"), "tag should point to new digest, got: {target_iri}");
    }

    #[tokio::test]
    async fn test_project_registry_gc_started() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();

        let event = EventEnvelope::new(
            iri_builder.event_schema("RegistryGCStarted", 1),
            "RegistryGCStarted",
            iri_builder.cluster_root(),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "scheduled_at": "2026-04-10T12:00:00Z",
            }),
        );
        projector.project(&event).await.unwrap();

        // Check GC resource was created with running status
        let result = projector
            .query(&format!(
                "SELECT ?gc ?status WHERE {{ ?gc <{RDF_TYPE}> <{PICLOUD_NS}RegistryGC> . ?gc <{PICLOUD_NS}status> ?status }}"
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(result.bindings[0]["status"]["value"].as_str().unwrap(), "running");

        // Check scheduled_at
        let result = projector
            .query(&format!(
                "SELECT ?at WHERE {{ ?gc <{RDF_TYPE}> <{PICLOUD_NS}RegistryGC> . ?gc <{PICLOUD_NS}scheduledAt> ?at }}"
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(result.bindings[0]["at"]["value"].as_str().unwrap(), "2026-04-10T12:00:00Z");
    }

    #[tokio::test]
    async fn test_project_registry_gc_completed() {
        let projector = OxigraphProjector::new().unwrap();
        let iri_builder = make_iri_builder();

        let correlation = Uuid::new_v4();

        // Start GC
        let start_event = EventEnvelope::new(
            iri_builder.event_schema("RegistryGCStarted", 1),
            "RegistryGCStarted",
            iri_builder.cluster_root(),
            None,
            correlation,
            serde_json::json!({
                "scheduled_at": "2026-04-10T12:00:00Z",
            }),
        );
        projector.project(&start_event).await.unwrap();

        // Complete GC
        let complete_event = EventEnvelope::new(
            iri_builder.event_schema("RegistryGCCompleted", 1),
            "RegistryGCCompleted",
            iri_builder.cluster_root(),
            None,
            correlation,
            serde_json::json!({
                "blobs_deleted": 42_u64,
                "bytes_reclaimed": 1073741824_u64,
                "duration_ms": 3500_u64,
            }),
        );
        projector.project(&complete_event).await.unwrap();

        // Status should be picloud:Completed (named node)
        let result = projector
            .query(&format!(
                "SELECT ?status WHERE {{ ?gc <{RDF_TYPE}> <{PICLOUD_NS}RegistryGC> . ?gc <{PICLOUD_NS}status> ?status }}"
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(result.bindings[0]["status"]["value"].as_str().unwrap(), &format!("{PICLOUD_NS}Completed"));

        // Check blobs_deleted and bytes_reclaimed
        let result = projector
            .query(&format!(
                "SELECT ?blobs ?bytes WHERE {{ ?gc <{RDF_TYPE}> <{PICLOUD_NS}RegistryGC> . ?gc <{PICLOUD_NS}blobsDeleted> ?blobs . ?gc <{PICLOUD_NS}bytesReclaimed> ?bytes }}"
            ))
            .await
            .unwrap();
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(result.bindings[0]["blobs"]["value"].as_str().unwrap(), "42");
        assert_eq!(result.bindings[0]["bytes"]["value"].as_str().unwrap(), "1073741824");
    }

    // --- CONSTRUCT/DESCRIBE serialization tests (ADR-005) ---

    fn make_event_generic(event_type: &str, payload: serde_json::Value) -> EventEnvelope {
        let iri_builder = make_iri_builder();
        let source = payload.get("node_iri")
            .or_else(|| payload.get("product_iri"))
            .or_else(|| payload.get("resource_iri"))
            .and_then(|v| v.as_str())
            .and_then(|s| ResourceIri::new(s).ok())
            .unwrap_or_else(|| iri_builder.cluster_root());
        let product = payload.get("product").and_then(|v| v.as_str()).map(String::from)
            .or_else(|| payload.get("product_name").and_then(|v| v.as_str()).map(String::from));
        EventEnvelope::new(
            iri_builder.event_schema(event_type, 1),
            event_type,
            source,
            product,
            Uuid::new_v4(),
            payload,
        )
    }

    #[tokio::test]
    async fn construct_query_returns_triples() {
        let projector = OxigraphProjector::new().unwrap();
        let event = make_event_generic("NodeJoined", serde_json::json!({
            "node_id": "test-node-id",
            "node_name": "test-node",
            "node_iri": "https://picloud.local/nodes/test-node",
            "address": "192.168.1.1:7443",
        }));
        projector.project(&event).await.unwrap();

        let result = projector.execute_query(
            "CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o . FILTER(CONTAINS(STR(?s), 'test-node')) } LIMIT 10"
        ).unwrap();
        assert!(!result.bindings.is_empty(), "CONSTRUCT should return triples");
        let first = &result.bindings[0];
        assert!(first.get("subject").is_some());
        assert!(first.get("predicate").is_some());
        assert!(first.get("object").is_some());
    }

    #[tokio::test]
    async fn construct_to_turtle_returns_valid_turtle() {
        let projector = OxigraphProjector::new().unwrap();
        let event = make_event_generic("NodeJoined", serde_json::json!({
            "node_id": "turtle-test-id",
            "node_name": "turtle-node",
            "node_iri": "https://picloud.local/nodes/turtle-node",
            "address": "10.0.0.1:7443",
        }));
        projector.project(&event).await.unwrap();

        let turtle = projector.execute_query_to_turtle(
            "CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o . FILTER(CONTAINS(STR(?s), 'turtle-node')) } LIMIT 10"
        ).unwrap();
        assert!(!turtle.trim().is_empty(), "Turtle output should not be empty");
        assert!(turtle.contains("turtle-node"), "Turtle should contain node name");
    }

    #[tokio::test]
    async fn remove_triple_removes_specific_predicate() {
        let projector = OxigraphProjector::new().unwrap();
        let event = make_event_generic("ProductDeployed", serde_json::json!({
            "product_iri": "https://picloud.local/products/remove-test",
            "product_name": "remove-test",
            "version": "1.0.0",
        }));
        projector.project(&event).await.unwrap();

        let result = projector.execute_query(
            &format!("SELECT ?v WHERE {{ <https://picloud.local/products/remove-test> <{PICLOUD_NS}version> ?v }}")
        ).unwrap();
        assert_eq!(result.bindings.len(), 1);

        projector.remove_triple(
            "https://picloud.local/products/remove-test",
            &format!("{PICLOUD_NS}version"),
        ).unwrap();

        let result = projector.execute_query(
            &format!("SELECT ?v WHERE {{ <https://picloud.local/products/remove-test> <{PICLOUD_NS}version> ?v }}")
        ).unwrap();
        assert!(result.bindings.is_empty());

        let result = projector.execute_query(
            &format!("SELECT ?t WHERE {{ <https://picloud.local/products/remove-test> <{PICLOUD_NS}name> ?t }}")
        ).unwrap();
        assert!(!result.bindings.is_empty(), "Other triples should survive removal");
    }

    // --- Upgrade projection tests (ADR-017) ---

    #[tokio::test]
    async fn project_product_upgrade_started_sets_state() {
        let projector = OxigraphProjector::new().unwrap();
        let deploy = make_event_generic("ProductDeployed", serde_json::json!({
            "product_iri": "https://picloud.local/products/upgrade-test",
            "product_name": "upgrade-test",
            "version": "1.0.0",
        }));
        projector.project(&deploy).await.unwrap();

        let upgrade = make_event_generic("ProductUpgradeStarted", serde_json::json!({
            "product_iri": "https://picloud.local/products/upgrade-test",
            "from_version": "1.0.0",
            "to_version": "2.0.0",
        }));
        projector.project(&upgrade).await.unwrap();

        let result = projector.execute_query(
            &format!("SELECT ?state WHERE {{ <https://picloud.local/products/upgrade-test> <{PICLOUD_NS}upgradeState> ?state }}")
        ).unwrap();
        assert_eq!(result.bindings[0]["state"]["value"].as_str().unwrap(), "Provisioning");
    }

    #[tokio::test]
    async fn project_product_upgrade_completed_updates_version() {
        let projector = OxigraphProjector::new().unwrap();
        let deploy = make_event_generic("ProductDeployed", serde_json::json!({
            "product_iri": "https://picloud.local/products/upg-done",
            "product_name": "upg-done",
            "version": "1.0.0",
        }));
        projector.project(&deploy).await.unwrap();
        let complete = make_event_generic("ProductUpgradeCompleted", serde_json::json!({
            "product_iri": "https://picloud.local/products/upg-done",
            "to_version": "2.0.0",
        }));
        projector.project(&complete).await.unwrap();

        let result = projector.execute_query(
            &format!("SELECT ?v WHERE {{ <https://picloud.local/products/upg-done> <{PICLOUD_NS}version> ?v }}")
        ).unwrap();
        assert_eq!(result.bindings[0]["v"]["value"].as_str().unwrap(), "2.0.0");
        let result = projector.execute_query(
            &format!("SELECT ?s WHERE {{ <https://picloud.local/products/upg-done> <{PICLOUD_NS}upgradeState> ?s }}")
        ).unwrap();
        assert!(result.bindings.is_empty());
    }

    // --- NetworkPolicy projection test (ADR-028) ---

    #[tokio::test]
    async fn product_deployed_creates_network_policy() {
        let projector = OxigraphProjector::new().unwrap();
        let event = make_event_generic("ProductDeployed", serde_json::json!({
            "product_iri": "https://picloud.local/products/net-test",
            "product_name": "net-test",
            "version": "1.0.0",
        }));
        projector.project(&event).await.unwrap();

        let result = projector.execute_query(
            &format!("SELECT ?type WHERE {{ <https://picloud.local/products/net-test/network-policy> <{RDF_TYPE}> ?type }}")
        ).unwrap();
        assert!(!result.bindings.is_empty(), "NetworkPolicy should exist");
    }

    // --- EventSubscription projection test (ADR-018) ---

    #[tokio::test]
    async fn event_subscription_projects_specific_triples() {
        let projector = OxigraphProjector::new().unwrap();
        let event = make_event_generic("ResourceDeclared", serde_json::json!({
            "resource_iri": "https://picloud.local/products/sub-test/subscriptions/my-sub",
            "resource_type": "EventSubscription",
            "product": "sub-test",
            "name": "my-sub",
            "source_product": "producer-app",
            "event_type": "OrderPlaced",
            "handler": "handle_order",
        }));
        projector.project(&event).await.unwrap();

        let result = projector.execute_query(
            &format!("SELECT ?src WHERE {{ <https://picloud.local/products/sub-test/subscriptions/my-sub> <{PICLOUD_NS}sourceProduct> ?src }}")
        ).unwrap();
        assert_eq!(result.bindings[0]["src"]["value"].as_str().unwrap(), "producer-app");
    }

    // --- Versioned ontology IRI test (ADR-023) ---

    #[tokio::test]
    async fn product_deployed_projects_versioned_ontology_iri() {
        let projector = OxigraphProjector::new().unwrap();
        let event = make_event_generic("ProductDeployed", serde_json::json!({
            "product_iri": "https://picloud.local/products/onto-test",
            "product_name": "onto-test",
            "version": "3.0.0",
        }));
        projector.project(&event).await.unwrap();

        let result = projector.execute_query(
            &format!("SELECT ?o WHERE {{ <https://picloud.local/products/onto-test> <{PICLOUD_NS}ontologyIri> ?o }}")
        ).unwrap();
        assert!(!result.bindings.is_empty());
        let iri = result.bindings[0]["o"]["value"].as_str().unwrap();
        assert!(iri.contains("ontology/3.0.0"), "Ontology IRI should contain version: {iri}");
    }
}
