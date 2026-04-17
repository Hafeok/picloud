//! DataProductProjectionRunner — subscribes to platform events and runs
//! SPARQL CONSTRUCT projections when declared trigger events fire (FT-067,
//! ADR-056).
//!
//! The runner holds a registry of `(data product → triggers + CONSTRUCT
//! query + source graph)` built up from `DataProductDeclared` /
//! `DataProductUpdated` / `DataProductDeleted` events (or via direct
//! `register` calls in tests). When an event arrives whose
//! `event_type` matches any registered trigger, the runner runs every
//! matching data product's CONSTRUCT projection in turn and emits a
//! `DataProductRefreshed` event (on success) or a
//! `DataProductProjectionFailed` event (on CONSTRUCT error).
//!
//! Push-only model: projections never run on a timer and never run on
//! query. Every rebuild is caused by a declared trigger event. This
//! matches the ADR-056 rationale — producers are forced to reason about
//! which domain events invalidate each analytical projection at design
//! time instead of discovering missing invalidations in production.
//!
//! Concurrency: multiple registrations may share the same trigger event.
//! In that case the runner iterates over them sequentially — each
//! projection is its own atomic swap, independent of the others. A
//! failing CONSTRUCT for one data product does not block refreshes for
//! the next.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::Utc;
use tracing::{debug, error, info, warn};

use picloud_domain::error::Result;
use picloud_domain::events::{
    DataProductProjectionFailedPayload, DataProductRefreshedPayload, EventEnvelope,
};
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::DataProductProjector;

/// Identification + projection metadata for a single registered data
/// product. The runner stores one of these per declared data product
/// and looks them up by trigger event name.
#[derive(Debug, Clone)]
pub struct DataProductRegistration {
    /// The data product's resource IRI (NOT the graph IRI — the graph is
    /// derived by the underlying `DataProductProjector`).
    pub data_product_iri: ResourceIri,
    /// Owning product name (for event scoping).
    pub product: String,
    /// Short name of the data product (e.g. "photo-locations").
    pub name: String,
    /// Event types that should trigger a projection rebuild. At least one
    /// trigger is required per ADR-056.
    pub triggers: Vec<String>,
    /// The SPARQL CONSTRUCT query text to execute on trigger.
    pub projection_query: String,
    /// The source (operational) named graph the CONSTRUCT reads from.
    /// Stored so the runner can pass it to the projector — the CONSTRUCT
    /// itself typically references the same IRI in a `GRAPH <...>` clause.
    pub source_graph_iri: ResourceIri,
}

impl DataProductRegistration {
    /// Convenience constructor for the common case where the source graph
    /// is simply the owning product's internal graph.
    pub fn new(
        data_product_iri: ResourceIri,
        product: impl Into<String>,
        name: impl Into<String>,
        triggers: Vec<String>,
        projection_query: impl Into<String>,
        source_graph_iri: ResourceIri,
    ) -> Self {
        Self {
            data_product_iri,
            product: product.into(),
            name: name.into(),
            triggers,
            projection_query: projection_query.into(),
            source_graph_iri,
        }
    }
}

/// Outcome of running a single projection on behalf of a trigger event.
#[derive(Debug, Clone)]
pub enum ProjectionOutcome {
    /// CONSTRUCT succeeded, data product graph atomically swapped.
    Refreshed {
        /// The `DataProductRefreshed` event that should be appended to
        /// the event log.
        event: EventEnvelope,
        /// Number of triples in the new projection.
        triple_count: u64,
        /// Duration of the CONSTRUCT + swap.
        duration_ms: u64,
    },
    /// CONSTRUCT failed; live graph left untouched.
    Failed {
        /// The `DataProductProjectionFailed` event to append.
        event: EventEnvelope,
        /// Human-readable failure reason.
        reason: String,
    },
}

impl ProjectionOutcome {
    pub fn event(&self) -> &EventEnvelope {
        match self {
            ProjectionOutcome::Refreshed { event, .. } => event,
            ProjectionOutcome::Failed { event, .. } => event,
        }
    }

    pub fn is_refreshed(&self) -> bool {
        matches!(self, ProjectionOutcome::Refreshed { .. })
    }
}

/// Projection runner — matches incoming events to registered data product
/// triggers and dispatches CONSTRUCT refreshes (FT-067).
pub struct DataProductProjectionRunner {
    projector: Arc<dyn DataProductProjector>,
    iri_builder: IriBuilder,
    /// Registration table keyed by the data product resource IRI.
    registrations: Arc<RwLock<HashMap<String, DataProductRegistration>>>,
    /// Reverse index: trigger event type → list of data product IRIs.
    /// Kept in lockstep with `registrations` under the same RwLock
    /// boundary so reads are consistent.
    trigger_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl DataProductProjectionRunner {
    /// Create a new runner that delegates CONSTRUCT execution to the
    /// provided projector.
    pub fn new(projector: Arc<dyn DataProductProjector>) -> Self {
        Self::with_domain(projector, ClusterDomain::default())
    }

    /// Create a new runner with a custom cluster domain (useful in tests
    /// and when the platform is running under a non-default DNS domain).
    pub fn with_domain(projector: Arc<dyn DataProductProjector>, domain: ClusterDomain) -> Self {
        Self {
            projector,
            iri_builder: IriBuilder::new(domain),
            registrations: Arc::new(RwLock::new(HashMap::new())),
            trigger_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add or replace a data product registration. If a registration with
    /// the same `data_product_iri` already exists, it is replaced and its
    /// old triggers are unsubscribed first.
    pub fn register(&self, reg: DataProductRegistration) {
        if reg.triggers.is_empty() {
            warn!(
                data_product = %reg.data_product_iri.as_str(),
                "Registering data product with no triggers — projections will never run"
            );
        }

        let key = reg.data_product_iri.as_str().to_string();

        // Drop the old registration's triggers first so we don't leak entries
        // in the reverse index when the set of triggers is narrowed.
        self.deregister_internal(&key);

        {
            let mut idx = self.trigger_index.write().expect("trigger_index poisoned");
            for trigger in &reg.triggers {
                idx.entry(trigger.clone())
                    .or_insert_with(Vec::new)
                    .push(key.clone());
            }
        }
        {
            let mut regs = self.registrations.write().expect("registrations poisoned");
            regs.insert(key.clone(), reg);
        }
        debug!(data_product = %key, "registered data product with projection runner");
    }

    /// Remove a data product's registration (on `DataProductDeleted` or
    /// when a `DataProductUpdated` replaces it).
    pub fn deregister(&self, data_product_iri: &ResourceIri) {
        self.deregister_internal(data_product_iri.as_str());
    }

    fn deregister_internal(&self, key: &str) {
        let old = {
            let mut regs = self.registrations.write().expect("registrations poisoned");
            regs.remove(key)
        };
        if let Some(old) = old {
            let mut idx = self.trigger_index.write().expect("trigger_index poisoned");
            for trigger in &old.triggers {
                if let Some(list) = idx.get_mut(trigger) {
                    list.retain(|k| k != key);
                    if list.is_empty() {
                        idx.remove(trigger);
                    }
                }
            }
            debug!(data_product = key, "deregistered data product");
        }
    }

    /// Number of currently registered data products (primarily for tests).
    pub fn registration_count(&self) -> usize {
        self.registrations
            .read()
            .expect("registrations poisoned")
            .len()
    }

    /// True if any registration is subscribed to the given trigger event.
    pub fn has_trigger(&self, event_type: &str) -> bool {
        self.trigger_index
            .read()
            .expect("trigger_index poisoned")
            .get(event_type)
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }

    /// Handle an incoming event.
    ///
    /// If the event's `event_type` matches any registered trigger, the
    /// runner runs the CONSTRUCT for each matching data product and returns
    /// one `ProjectionOutcome` per refresh attempt. Non-matching events
    /// return an empty Vec — they are not errors.
    ///
    /// This method is `async` because `DataProductProjector::refresh_projection`
    /// is async — the underlying Oxigraph operations are synchronous but
    /// the trait is defined in terms of `async_trait`.
    pub async fn handle_event(&self, event: &EventEnvelope) -> Result<Vec<ProjectionOutcome>> {
        // Copy the matching registrations out under the read lock so we
        // don't hold the lock across await points.
        let matches: Vec<DataProductRegistration> = {
            let idx = self.trigger_index.read().expect("trigger_index poisoned");
            let regs = self.registrations.read().expect("registrations poisoned");
            idx.get(&event.event_type)
                .map(|keys| {
                    keys.iter()
                        .filter_map(|k| regs.get(k).cloned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };

        if matches.is_empty() {
            return Ok(Vec::new());
        }

        info!(
            event_type = %event.event_type,
            data_products = matches.len(),
            "dispatching projection refreshes for trigger event"
        );

        let mut outcomes = Vec::with_capacity(matches.len());
        for reg in matches {
            let outcome = self.run_one(&reg, &event.event_type, event.correlation_id).await;
            outcomes.push(outcome);
        }
        Ok(outcomes)
    }

    /// Execute a single data product's CONSTRUCT projection and wrap the
    /// result in the right lifecycle event.
    async fn run_one(
        &self,
        reg: &DataProductRegistration,
        trigger_event: &str,
        correlation_id: uuid::Uuid,
    ) -> ProjectionOutcome {
        match self
            .projector
            .refresh_projection(
                &reg.data_product_iri,
                &reg.projection_query,
                &reg.source_graph_iri,
            )
            .await
        {
            Ok(refresh) => {
                let payload = DataProductRefreshedPayload {
                    data_product_iri: reg.data_product_iri.clone(),
                    triple_count: refresh.triple_count,
                    duration_ms: refresh.duration_ms,
                    trigger_event: trigger_event.to_string(),
                    refreshed_at: Utc::now(),
                };
                let event = EventEnvelope::new(
                    self.iri_builder.event_schema("DataProductRefreshed", 1),
                    "DataProductRefreshed",
                    reg.data_product_iri.clone(),
                    Some(reg.product.clone()),
                    correlation_id,
                    serde_json::to_value(&payload).unwrap_or_else(|_| serde_json::json!({
                        "data_product_iri": reg.data_product_iri.as_str(),
                        "triple_count": refresh.triple_count,
                        "duration_ms": refresh.duration_ms,
                        "trigger_event": trigger_event,
                        "refreshed_at": Utc::now().to_rfc3339(),
                    })),
                );
                info!(
                    data_product = %reg.data_product_iri.as_str(),
                    trigger = trigger_event,
                    triple_count = refresh.triple_count,
                    duration_ms = refresh.duration_ms,
                    "projection refresh succeeded"
                );
                ProjectionOutcome::Refreshed {
                    event,
                    triple_count: refresh.triple_count,
                    duration_ms: refresh.duration_ms,
                }
            }
            Err(err) => {
                let reason = err.to_string();
                error!(
                    data_product = %reg.data_product_iri.as_str(),
                    trigger = trigger_event,
                    error = %reason,
                    "projection refresh failed — live graph left unchanged"
                );
                let payload = DataProductProjectionFailedPayload {
                    data_product_iri: reg.data_product_iri.clone(),
                    trigger_event: trigger_event.to_string(),
                    reason: reason.clone(),
                    failed_at: Utc::now(),
                };
                let event = EventEnvelope::new(
                    self.iri_builder
                        .event_schema("DataProductProjectionFailed", 1),
                    "DataProductProjectionFailed",
                    reg.data_product_iri.clone(),
                    Some(reg.product.clone()),
                    correlation_id,
                    serde_json::to_value(&payload).unwrap_or_else(|_| serde_json::json!({
                        "data_product_iri": reg.data_product_iri.as_str(),
                        "trigger_event": trigger_event,
                        "reason": reason,
                        "failed_at": Utc::now().to_rfc3339(),
                    })),
                );
                ProjectionOutcome::Failed { event, reason }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OxigraphDataProductProjector, OxigraphProjector};
    use oxigraph::model::{Literal, NamedNode, NamedNodeRef, QuadRef};
    use std::sync::Arc;
    use uuid::Uuid;

    const PICLOUD_NS: &str = "https://picloud.local/ontology#";
    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

    fn iri_builder() -> IriBuilder {
        IriBuilder::new(ClusterDomain::default())
    }

    fn seed_photo(store: &oxigraph::store::Store, pg: &str, iri: &str, place: &str, lat: &str, lon: &str) {
        store
            .insert_named_graph(NamedNodeRef::new(pg).unwrap())
            .unwrap();
        let g = NamedNode::new(pg).unwrap();
        let photo = NamedNode::new(iri).unwrap();
        let type_pred = NamedNode::new(RDF_TYPE).unwrap();
        let photo_type = NamedNode::new(&format!("{PICLOUD_NS}Photo")).unwrap();
        store
            .insert(QuadRef::new(&photo, &type_pred, &photo_type, &g))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo,
                &NamedNode::new(&format!("{PICLOUD_NS}placeName")).unwrap(),
                &Literal::new_simple_literal(place),
                &g,
            ))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo,
                &NamedNode::new(&format!("{PICLOUD_NS}latitude")).unwrap(),
                &Literal::new_simple_literal(lat),
                &g,
            ))
            .unwrap();
        store
            .insert(QuadRef::new(
                &photo,
                &NamedNode::new(&format!("{PICLOUD_NS}longitude")).unwrap(),
                &Literal::new_simple_literal(lon),
                &g,
            ))
            .unwrap();
    }

    fn make_trigger_event(event_type: &str) -> EventEnvelope {
        let ib = iri_builder();
        EventEnvelope::new(
            ib.event_schema(event_type, 1),
            event_type,
            ib.product("photo-app"),
            Some("photo-app".to_string()),
            Uuid::new_v4(),
            serde_json::json!({ "trigger": event_type }),
        )
    }

    #[tokio::test]
    async fn unregistered_trigger_produces_no_outcomes() {
        let projector = OxigraphProjector::new().unwrap();
        let dp_proj: Arc<dyn DataProductProjector> = Arc::new(
            OxigraphDataProductProjector::new(Arc::new(projector.store().clone())),
        );
        let runner = DataProductProjectionRunner::new(dp_proj);

        let event = make_trigger_event("SomethingRandom");
        let outcomes = runner.handle_event(&event).await.unwrap();
        assert!(outcomes.is_empty());
        assert!(!runner.has_trigger("SomethingRandom"));
    }

    #[tokio::test]
    async fn registered_trigger_runs_projection_and_emits_refreshed_event() {
        let ib = iri_builder();
        let projector = OxigraphProjector::new().unwrap();
        let product_graph = ib.product_graph("photo-app");
        seed_photo(
            projector.store(),
            product_graph.as_str(),
            "https://picloud.local/products/photo-app/photos/p1",
            "Paris",
            "48.8566",
            "2.3522",
        );

        let dp_proj_concrete =
            Arc::new(OxigraphDataProductProjector::new(Arc::new(projector.store().clone())));
        let runner = DataProductProjectionRunner::new(
            dp_proj_concrete.clone() as Arc<dyn DataProductProjector>,
        );

        let dp_graph = ib.data_product_graph("photo-app", "photo-locations");
        let dp_iri_str = dp_graph.as_str().trim_end_matches("/graph").to_string();
        let dp_iri = ResourceIri::new(&dp_iri_str).unwrap();

        let construct = format!(
            r#"CONSTRUCT {{
                ?photo <{PICLOUD_NS}placeName> ?place .
                ?photo <{PICLOUD_NS}latitude> ?lat .
                ?photo <{PICLOUD_NS}longitude> ?lon .
            }} WHERE {{
                GRAPH <{pg}> {{
                    ?photo a <{PICLOUD_NS}Photo> ;
                           <{PICLOUD_NS}placeName> ?place ;
                           <{PICLOUD_NS}latitude> ?lat ;
                           <{PICLOUD_NS}longitude> ?lon .
                }}
            }}"#,
            pg = product_graph.as_str(),
        );

        runner.register(DataProductRegistration::new(
            dp_iri.clone(),
            "photo-app",
            "photo-locations",
            vec!["PlaceResolved".to_string()],
            construct,
            product_graph.clone(),
        ));

        assert_eq!(runner.registration_count(), 1);
        assert!(runner.has_trigger("PlaceResolved"));

        let event = make_trigger_event("PlaceResolved");
        let outcomes = runner.handle_event(&event).await.unwrap();
        assert_eq!(outcomes.len(), 1);
        let outcome = &outcomes[0];
        match outcome {
            ProjectionOutcome::Refreshed { event: e, triple_count, duration_ms } => {
                assert_eq!(e.event_type, "DataProductRefreshed");
                assert_eq!(*triple_count, 3);
                // duration_ms is non-negative by construction (u64); sanity check < 5s.
                assert!(*duration_ms < 5_000);
                assert_eq!(e.payload["trigger_event"], "PlaceResolved");
                assert_eq!(e.payload["triple_count"], 3);
                assert_eq!(e.correlation_id, event.correlation_id);
                assert_eq!(e.product.as_deref(), Some("photo-app"));
            }
            other => panic!("expected Refreshed outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn malformed_construct_emits_projection_failed() {
        let ib = iri_builder();
        let projector = OxigraphProjector::new().unwrap();
        let dp_proj: Arc<dyn DataProductProjector> = Arc::new(
            OxigraphDataProductProjector::new(Arc::new(projector.store().clone())),
        );
        let runner = DataProductProjectionRunner::new(dp_proj);

        let dp_iri = ResourceIri::new(
            ib.data_product_graph("photo-app", "broken")
                .as_str()
                .trim_end_matches("/graph"),
        )
        .unwrap();

        runner.register(DataProductRegistration::new(
            dp_iri.clone(),
            "photo-app",
            "broken",
            vec!["PlaceResolved".to_string()],
            "THIS IS NOT VALID SPARQL",
            ib.product_graph("photo-app"),
        ));

        let event = make_trigger_event("PlaceResolved");
        let outcomes = runner.handle_event(&event).await.unwrap();
        assert_eq!(outcomes.len(), 1);
        match &outcomes[0] {
            ProjectionOutcome::Failed { event: e, reason } => {
                assert_eq!(e.event_type, "DataProductProjectionFailed");
                assert!(!reason.is_empty());
                assert_eq!(e.payload["trigger_event"], "PlaceResolved");
            }
            other => panic!("expected Failed outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn deregister_stops_future_triggers() {
        let ib = iri_builder();
        let projector = OxigraphProjector::new().unwrap();
        let dp_proj: Arc<dyn DataProductProjector> = Arc::new(
            OxigraphDataProductProjector::new(Arc::new(projector.store().clone())),
        );
        let runner = DataProductProjectionRunner::new(dp_proj);

        let dp_iri = ResourceIri::new(
            ib.data_product_graph("photo-app", "dp1")
                .as_str()
                .trim_end_matches("/graph"),
        )
        .unwrap();

        runner.register(DataProductRegistration::new(
            dp_iri.clone(),
            "photo-app",
            "dp1",
            vec!["PlaceResolved".to_string()],
            "CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }",
            ib.product_graph("photo-app"),
        ));
        assert_eq!(runner.registration_count(), 1);
        runner.deregister(&dp_iri);
        assert_eq!(runner.registration_count(), 0);
        assert!(!runner.has_trigger("PlaceResolved"));

        let outcomes = runner.handle_event(&make_trigger_event("PlaceResolved")).await.unwrap();
        assert!(outcomes.is_empty());
    }
}
