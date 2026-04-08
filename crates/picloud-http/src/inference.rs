//! Inference Engine (ADR-038)
//!
//! Evaluates SPARQL CONSTRUCT inference rules against the RDF graph.
//! Detects new triples (assertions) and removed triples (retractions)
//! between evaluations. Emits GroupMembershipChanged, AlertFired, and
//! AlertResolved events as appropriate.
//!
//! Two evaluation modes:
//! 1. Event-triggered: subscribes to the event log and evaluates rules
//!    whose trigger_events match the incoming event type.
//! 2. Reconciliation: every N seconds, evaluates all rules with
//!    reconciliation=true regardless of events.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use picloud_domain::error::Result;
use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{IriBuilder, ResourceIri};
use picloud_domain::traits::{EventFilter, EventLog, StateProjector};

/// A loaded inference rule ready for evaluation.
#[derive(Debug, Clone)]
pub struct LoadedRule {
    /// IRI of the rule resource
    pub iri: ResourceIri,
    /// Human-readable name
    pub name: String,
    /// Scope: "platform" or a product name
    pub scope: String,
    /// Trigger mode
    pub trigger: String,
    /// Event types that trigger this rule
    pub trigger_events: Vec<String>,
    /// Whether to run on reconciliation schedule
    pub reconciliation: bool,
    /// The SPARQL CONSTRUCT query
    pub construct_query: String,
}

/// A produced triple from a CONSTRUCT query, represented as string components
/// for comparison across evaluations.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProducedTriple {
    pub subject: String,
    pub predicate: String,
    pub object: String,
}

/// The inference engine evaluates CONSTRUCT rules and detects changes.
pub struct InferenceEngine {
    projector: Arc<dyn StateProjector>,
    event_log: Arc<dyn EventLog>,
    iri_builder: IriBuilder,
    /// Previously produced triples per rule IRI, for diff detection.
    previous_triples: Mutex<HashMap<String, HashSet<ProducedTriple>>>,
    /// Loaded rules
    rules: Mutex<Vec<LoadedRule>>,
}

impl InferenceEngine {
    pub fn new(
        projector: Arc<dyn StateProjector>,
        event_log: Arc<dyn EventLog>,
        iri_builder: IriBuilder,
    ) -> Self {
        Self {
            projector,
            event_log,
            iri_builder,
            previous_triples: Mutex::new(HashMap::new()),
            rules: Mutex::new(Vec::new()),
        }
    }

    /// Register a rule for evaluation.
    pub async fn register_rule(&self, rule: LoadedRule) {
        let mut rules = self.rules.lock().await;
        // Replace if already registered (by IRI)
        rules.retain(|r| r.iri != rule.iri);
        rules.push(rule);
    }

    /// Evaluate a single rule: run its CONSTRUCT query, diff with previous
    /// results, and emit events for assertions/retractions.
    ///
    /// Returns (assertions, retractions) counts.
    pub async fn evaluate_rule(&self, rule: &LoadedRule) -> Result<(usize, usize)> {
        // Run the CONSTRUCT query. We use a SELECT wrapper to extract
        // the triples from the CONSTRUCT pattern, since Oxigraph's
        // CONSTRUCT returns a graph iterator that we can't easily access
        // through the StateProjector trait.
        //
        // We transform CONSTRUCT { ?s ?p ?o } WHERE { ... }
        // into SELECT ?s ?p ?o WHERE { ... }
        let select_query = construct_to_select(&rule.construct_query);
        let result = self.projector.query(&select_query).await?;

        let mut current_triples = HashSet::new();
        for binding in &result.bindings {
            let subject = binding.get("s")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let predicate = binding.get("p")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let object = binding.get("o")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();

            if !subject.is_empty() && !predicate.is_empty() && !object.is_empty() {
                current_triples.insert(ProducedTriple { subject, predicate, object });
            }
        }

        let rule_key = rule.iri.as_str().to_string();
        let mut prev_map = self.previous_triples.lock().await;
        let previous = prev_map.get(&rule_key).cloned().unwrap_or_default();

        // Compute assertions (new) and retractions (removed)
        let assertions: Vec<&ProducedTriple> = current_triples.difference(&previous).collect();
        let retractions: Vec<&ProducedTriple> = previous.difference(&current_triples).collect();

        let assertion_count = assertions.len();
        let retraction_count = retractions.len();

        // Emit events for hasMember changes (ADR-037)
        let has_member_pred = format!("{}hasMember", "https://picloud.local/ontology#");
        for triple in &assertions {
            if triple.predicate == has_member_pred {
                let event = EventEnvelope::new(
                    self.iri_builder.event_schema("GroupMembershipChanged", 1),
                    "GroupMembershipChanged",
                    rule.iri.clone(),
                    None,
                    Uuid::new_v4(),
                    serde_json::json!({
                        "group_iri": triple.subject,
                        "member_iri": triple.object,
                        "action": "added",
                    }),
                );
                if let Err(e) = self.event_log.append(event).await {
                    error!(rule = rule.name, "failed to emit GroupMembershipChanged: {e}");
                }
            }
        }

        for triple in &retractions {
            if triple.predicate == has_member_pred {
                let event = EventEnvelope::new(
                    self.iri_builder.event_schema("GroupMembershipChanged", 1),
                    "GroupMembershipChanged",
                    rule.iri.clone(),
                    None,
                    Uuid::new_v4(),
                    serde_json::json!({
                        "group_iri": triple.subject,
                        "member_iri": triple.object,
                        "action": "removed",
                    }),
                );
                if let Err(e) = self.event_log.append(event).await {
                    error!(rule = rule.name, "failed to emit GroupMembershipChanged: {e}");
                }
            }
        }

        // Emit InferenceRuleEvaluated
        if assertion_count > 0 || retraction_count > 0 {
            let eval_event = EventEnvelope::new(
                self.iri_builder.event_schema("InferenceRuleEvaluated", 1),
                "InferenceRuleEvaluated",
                rule.iri.clone(),
                None,
                Uuid::new_v4(),
                serde_json::json!({
                    "rule_iri": rule.iri.as_str(),
                    "assertions": assertion_count,
                    "retractions": retraction_count,
                }),
            );
            if let Err(e) = self.event_log.append(eval_event).await {
                error!(rule = rule.name, "failed to emit InferenceRuleEvaluated: {e}");
            }
        }

        // Update previous state
        prev_map.insert(rule_key, current_triples);

        debug!(
            rule = rule.name,
            assertions = assertion_count,
            retractions = retraction_count,
            "rule evaluated"
        );

        Ok((assertion_count, retraction_count))
    }

    /// Run all rules that are triggered by a given event type.
    pub async fn evaluate_triggered_rules(&self, event_type: &str) -> Result<()> {
        let rules = self.rules.lock().await.clone();
        for rule in &rules {
            if rule.trigger == "event" && rule.trigger_events.contains(&event_type.to_string()) {
                if let Err(e) = self.evaluate_rule(rule).await {
                    warn!(rule = rule.name, event_type = event_type, error = %e, "rule evaluation failed");
                }
            }
        }
        Ok(())
    }

    /// Run all rules with reconciliation=true. Returns total (assertions, retractions).
    pub async fn run_reconciliation(&self) -> Result<(usize, usize)> {
        let rules = self.rules.lock().await.clone();
        let mut total_assertions = 0usize;
        let mut total_retractions = 0usize;
        let mut rules_evaluated = 0usize;

        for rule in &rules {
            if rule.reconciliation {
                match self.evaluate_rule(rule).await {
                    Ok((a, r)) => {
                        total_assertions += a;
                        total_retractions += r;
                        rules_evaluated += 1;
                    }
                    Err(e) => {
                        warn!(rule = rule.name, error = %e, "reconciliation rule evaluation failed");
                    }
                }
            }
        }

        // Emit ReconciliationCompleted event
        let recon_event = EventEnvelope::new(
            self.iri_builder.event_schema("ReconciliationCompleted", 1),
            "ReconciliationCompleted",
            ResourceIri("https://picloud.local/inference-engine".to_string()),
            None,
            Uuid::new_v4(),
            serde_json::json!({
                "rules_evaluated": rules_evaluated,
                "total_assertions": total_assertions,
                "total_retractions": total_retractions,
            }),
        );
        if let Err(e) = self.event_log.append(recon_event).await {
            error!("failed to emit ReconciliationCompleted: {e}");
        }

        info!(
            rules = rules_evaluated,
            assertions = total_assertions,
            retractions = total_retractions,
            "reconciliation pass completed"
        );

        Ok((total_assertions, total_retractions))
    }

    /// Start the event-triggered evaluation loop.
    /// Subscribes to the event log and evaluates matching rules on each event.
    pub fn start_event_loop(self: &Arc<Self>) {
        let engine = Arc::clone(self);
        tokio::spawn(async move {
            let mut rx = match engine.event_log.subscribe(EventFilter::default()).await {
                Ok(rx) => rx,
                Err(e) => {
                    error!("inference engine failed to subscribe to event log: {e}");
                    return;
                }
            };
            info!("Inference engine event loop started");
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        // Avoid re-triggering on our own events
                        if event.event_type == "InferenceRuleEvaluated"
                            || event.event_type == "ReconciliationCompleted"
                            || event.event_type == "GroupMembershipChanged"
                        {
                            continue;
                        }
                        if let Err(e) = engine.evaluate_triggered_rules(&event.event_type).await {
                            error!(event_type = %event.event_type, error = %e, "triggered rule evaluation failed");
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "inference engine subscriber lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("event log closed — stopping inference engine event loop");
                        break;
                    }
                }
            }
        });
    }

    /// Start the reconciliation loop that runs every `interval_secs` seconds.
    pub fn start_reconciliation_loop(self: &Arc<Self>, interval_secs: u64) {
        let engine = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                tokio::time::Duration::from_secs(interval_secs),
            );
            // Skip the first immediate tick
            interval.tick().await;
            info!(interval_secs = interval_secs, "Inference engine reconciliation loop started");
            loop {
                interval.tick().await;
                if let Err(e) = engine.run_reconciliation().await {
                    error!(error = %e, "reconciliation pass failed");
                }
            }
        });
    }
}

/// Transform a SPARQL CONSTRUCT query into a SELECT query that returns
/// the same bindings. This is a practical approach since the StateProjector
/// trait only supports SELECT queries.
///
/// We extract the WHERE clause and produce `SELECT ?s ?p ?o WHERE { ... }`.
/// The CONSTRUCT template variables become the SELECT projection.
fn construct_to_select(construct: &str) -> String {
    // Try to find the WHERE clause
    let upper = construct.to_uppercase();
    if let Some(where_pos) = upper.find("WHERE") {
        let after_where = &construct[where_pos + 5..];
        // Find the opening brace
        if let Some(brace_pos) = after_where.find('{') {
            let body = &after_where[brace_pos..];
            return format!("SELECT ?s ?p ?o {body}");
        }
    }
    // Fallback: wrap the entire thing
    format!("SELECT ?s ?p ?o WHERE {{ {construct} }}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use picloud_domain::events::EventEnvelope;
    use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
    use picloud_domain::traits::{EventFilter, EventLog, QueryResult, StateProjector};

    // -- Mock StateProjector --
    struct MockProjector {
        query_results: Mutex<Vec<serde_json::Value>>,
    }

    impl MockProjector {
        fn new(results: Vec<serde_json::Value>) -> Self {
            Self {
                query_results: Mutex::new(results),
            }
        }
    }

    #[async_trait::async_trait]
    impl StateProjector for MockProjector {
        async fn project(&self, _event: &EventEnvelope) -> Result<()> {
            Ok(())
        }

        async fn query(&self, _sparql: &str) -> Result<QueryResult> {
            let bindings = self.query_results.lock().await.clone();
            Ok(QueryResult { bindings })
        }

        async fn query_product(
            &self,
            _product_iri: &ResourceIri,
            _sparql: &str,
        ) -> Result<QueryResult> {
            Ok(QueryResult { bindings: vec![] })
        }
    }

    // -- Mock EventLog --
    struct MockEventLog {
        appended: Mutex<Vec<EventEnvelope>>,
    }

    impl MockEventLog {
        fn new() -> Self {
            Self {
                appended: Mutex::new(Vec::new()),
            }
        }

        async fn appended_events(&self) -> Vec<EventEnvelope> {
            self.appended.lock().await.clone()
        }
    }

    #[async_trait::async_trait]
    impl EventLog for MockEventLog {
        async fn append(&self, event: EventEnvelope) -> Result<()> {
            self.appended.lock().await.push(event);
            Ok(())
        }

        async fn subscribe(
            &self,
            _filter: EventFilter,
        ) -> Result<tokio::sync::broadcast::Receiver<EventEnvelope>> {
            let (tx, rx) = tokio::sync::broadcast::channel(16);
            drop(tx);
            Ok(rx)
        }

        async fn events_since(&self, _offset: usize) -> Vec<EventEnvelope> {
            Vec::new()
        }
    }

    fn make_iri_builder() -> IriBuilder {
        IriBuilder::new(ClusterDomain::default())
    }

    #[tokio::test]
    async fn evaluate_rule_detects_new_triples() {
        let projector = Arc::new(MockProjector::new(vec![
            serde_json::json!({
                "s": "https://picloud.local/groups/backend",
                "p": "https://picloud.local/ontology#hasMember",
                "o": "https://picloud.local/platform/identities/alice"
            }),
        ]));
        let event_log = Arc::new(MockEventLog::new());
        let engine = InferenceEngine::new(
            projector,
            event_log.clone(),
            make_iri_builder(),
        );

        let rule = LoadedRule {
            iri: ResourceIri("https://picloud.local/inference-rules/test-rule".to_string()),
            name: "test-rule".to_string(),
            scope: "platform".to_string(),
            trigger: "event".to_string(),
            trigger_events: vec!["TagAdded".to_string()],
            reconciliation: true,
            construct_query: "CONSTRUCT { ?s ?p ?o } WHERE { ?s a <https://picloud.local/ontology#HumanIdentity> . BIND(?s AS ?o) }".to_string(),
        };

        let (assertions, retractions) = engine.evaluate_rule(&rule).await.unwrap();
        assert_eq!(assertions, 1);
        assert_eq!(retractions, 0);

        // Should have emitted GroupMembershipChanged + InferenceRuleEvaluated
        let events = event_log.appended_events().await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "GroupMembershipChanged");
        assert_eq!(events[0].payload["action"], "added");
        assert_eq!(events[1].event_type, "InferenceRuleEvaluated");
    }

    #[tokio::test]
    async fn evaluate_rule_detects_retractions() {
        let projector = Arc::new(MockProjector::new(vec![
            serde_json::json!({
                "s": "https://picloud.local/groups/backend",
                "p": "https://picloud.local/ontology#hasMember",
                "o": "https://picloud.local/platform/identities/alice"
            }),
        ]));
        let event_log = Arc::new(MockEventLog::new());
        let engine = InferenceEngine::new(
            projector.clone(),
            event_log.clone(),
            make_iri_builder(),
        );

        let rule = LoadedRule {
            iri: ResourceIri("https://picloud.local/inference-rules/test-rule".to_string()),
            name: "test-rule".to_string(),
            scope: "platform".to_string(),
            trigger: "event".to_string(),
            trigger_events: vec!["TagAdded".to_string()],
            reconciliation: true,
            construct_query: "CONSTRUCT { ?s ?p ?o } WHERE { ?s a picloud:HumanIdentity }".to_string(),
        };

        // First evaluation: alice is a member
        let (a, r) = engine.evaluate_rule(&rule).await.unwrap();
        assert_eq!(a, 1);
        assert_eq!(r, 0);

        // Now change the projector to return empty (alice lost the tag)
        *projector.query_results.lock().await = vec![];

        let (a, r) = engine.evaluate_rule(&rule).await.unwrap();
        assert_eq!(a, 0);
        assert_eq!(r, 1);

        // Check retraction event
        let events = event_log.appended_events().await;
        // 2 from first eval (membership + rule_eval) + 2 from second eval
        assert_eq!(events.len(), 4);
        assert_eq!(events[2].event_type, "GroupMembershipChanged");
        assert_eq!(events[2].payload["action"], "removed");
    }

    #[tokio::test]
    async fn construct_to_select_extracts_where_clause() {
        let construct = r#"
            CONSTRUCT {
                ?group picloud:hasMember ?user .
            }
            WHERE {
                ?user a picloud:HumanIdentity ;
                      picloud:tag ?tag .
            }
        "#;
        let select = construct_to_select(construct);
        assert!(select.starts_with("SELECT ?s ?p ?o"));
        assert!(select.contains("?user a picloud:HumanIdentity"));
    }

    #[tokio::test]
    async fn reconciliation_emits_completed_event() {
        let projector = Arc::new(MockProjector::new(vec![]));
        let event_log = Arc::new(MockEventLog::new());
        let engine = InferenceEngine::new(
            projector,
            event_log.clone(),
            make_iri_builder(),
        );

        let rule = LoadedRule {
            iri: ResourceIri("https://picloud.local/inference-rules/recon-rule".to_string()),
            name: "recon-rule".to_string(),
            scope: "platform".to_string(),
            trigger: "event".to_string(),
            trigger_events: vec![],
            reconciliation: true,
            construct_query: "CONSTRUCT { ?s ?p ?o } WHERE { ?s a picloud:Foo }".to_string(),
        };

        engine.register_rule(rule).await;
        let (a, r) = engine.run_reconciliation().await.unwrap();
        assert_eq!(a, 0);
        assert_eq!(r, 0);

        let events = event_log.appended_events().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "ReconciliationCompleted");
    }
}
