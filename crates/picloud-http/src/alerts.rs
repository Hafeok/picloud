/// Built-in Alert Evaluator (ADR-041)
///
/// Evaluates platform alert rules against collected hardware metrics.
/// Tracks which alerts are currently firing to produce AlertFired events
/// when thresholds are exceeded and AlertResolved events when conditions
/// return to normal.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;

use picloud_domain::error::Result;
use picloud_domain::events::{
    AlertFiredPayload, AlertResolvedPayload, MetricEntry,
};
use picloud_domain::iri::{IriBuilder, ResourceIri};
use picloud_domain::resources::BuiltInAlertRule;
use picloud_domain::traits::{AlertAction, AlertEvaluator};

/// Key used to track firing state per (rule_name, resource) pair.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AlertKey {
    rule_name: String,
    resource_iri: String,
}

/// The built-in alert evaluator checks metrics against threshold-based rules
/// and emits AlertFired / AlertResolved actions.
pub struct BuiltInAlertEvaluator {
    rules: Vec<BuiltInAlertRule>,
    iri_builder: IriBuilder,
    /// Tracks which alerts are currently firing.
    /// Key: (rule_name, resource_iri) -> true if firing
    firing: Mutex<HashMap<AlertKey, bool>>,
}

impl BuiltInAlertEvaluator {
    pub fn new(rules: Vec<BuiltInAlertRule>, iri_builder: IriBuilder) -> Self {
        Self {
            rules,
            iri_builder,
            firing: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl AlertEvaluator for BuiltInAlertEvaluator {
    async fn evaluate(
        &self,
        node_iri: &ResourceIri,
        metrics: &[MetricEntry],
    ) -> Result<Vec<AlertAction>> {
        let mut actions = Vec::new();
        let mut firing = self.firing.lock().unwrap();
        let now = Utc::now();

        for rule in &self.rules {
            let key = AlertKey {
                rule_name: rule.name.to_string(),
                resource_iri: node_iri.as_str().to_string(),
            };

            // Find the metric that this rule checks
            let metric_value = metrics
                .iter()
                .find(|m| m.name == rule.metric_name)
                .map(|m| m.value);

            let Some(value) = metric_value else {
                // Metric not present in this collection — skip rule
                continue;
            };

            let exceeds_threshold = value > rule.threshold;
            let was_firing = firing.get(&key).copied().unwrap_or(false);

            if exceeds_threshold && !was_firing {
                // Transition: OK → FIRING
                let rule_iri = self.iri_builder.inference_rule(rule.name);
                let message = rule
                    .message_template
                    .replace("{value}", &format!("{value:.1}"))
                    .replace("{threshold}", &format!("{:.1}", rule.threshold));

                actions.push(AlertAction::Fire(AlertFiredPayload {
                    alert_type: rule.alert_type.to_string(),
                    severity: rule.severity.clone(),
                    message,
                    resource_iri: node_iri.clone(),
                    rule_iri: rule_iri.clone(),
                    fired_at: now,
                }));

                firing.insert(key, true);
            } else if !exceeds_threshold && was_firing {
                // Transition: FIRING → OK (resolved)
                let rule_iri = self.iri_builder.inference_rule(rule.name);

                actions.push(AlertAction::Resolve(AlertResolvedPayload {
                    alert_type: rule.alert_type.to_string(),
                    resource_iri: node_iri.clone(),
                    rule_iri,
                    resolved_at: now,
                }));

                firing.insert(key, false);
            }
            // If exceeds && was_firing → still firing, no new event (dampening)
            // If !exceeds && !was_firing → still OK, no event
        }

        Ok(actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picloud_domain::events::AlertSeverity;
    use picloud_domain::iri::ClusterDomain;
    use picloud_domain::resources::builtin_alert_rules;

    fn test_iri_builder() -> IriBuilder {
        IriBuilder::new(ClusterDomain::default())
    }

    fn test_node_iri() -> ResourceIri {
        test_iri_builder().node("test-node-01")
    }

    #[tokio::test]
    async fn fires_critical_alert_when_cpu_temp_exceeds_80() {
        let evaluator = BuiltInAlertEvaluator::new(builtin_alert_rules(), test_iri_builder());
        let node_iri = test_node_iri();

        let metrics = vec![MetricEntry {
            name: "cpu_temp_celsius".to_string(),
            value: 85.0,
            unit: "celsius".to_string(),
        }];

        let actions = evaluator.evaluate(&node_iri, &metrics).await.unwrap();

        // Should fire both warning (>70) and critical (>80)
        assert_eq!(actions.len(), 2);

        // Find the critical alert
        let critical = actions.iter().find(|a| matches!(a, AlertAction::Fire(p) if p.severity == AlertSeverity::Critical));
        assert!(critical.is_some(), "Expected a critical AlertFired action");

        if let Some(AlertAction::Fire(payload)) = critical {
            assert_eq!(payload.alert_type, "HighCpuTemperature");
            assert_eq!(payload.severity, AlertSeverity::Critical);
        }
    }

    #[tokio::test]
    async fn resolves_alert_when_temp_drops_below_threshold() {
        let evaluator = BuiltInAlertEvaluator::new(builtin_alert_rules(), test_iri_builder());
        let node_iri = test_node_iri();

        // First: fire the alert
        let hot_metrics = vec![MetricEntry {
            name: "cpu_temp_celsius".to_string(),
            value: 85.0,
            unit: "celsius".to_string(),
        }];
        let actions = evaluator.evaluate(&node_iri, &hot_metrics).await.unwrap();
        assert!(!actions.is_empty(), "Should fire alerts");

        // Second: resolve the alert
        let cool_metrics = vec![MetricEntry {
            name: "cpu_temp_celsius".to_string(),
            value: 55.0,
            unit: "celsius".to_string(),
        }];
        let actions = evaluator.evaluate(&node_iri, &cool_metrics).await.unwrap();

        // Should resolve both warning and critical
        let resolved: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, AlertAction::Resolve(_)))
            .collect();
        assert_eq!(resolved.len(), 2);
    }

    #[tokio::test]
    async fn no_duplicate_alerts_while_still_firing() {
        let evaluator = BuiltInAlertEvaluator::new(builtin_alert_rules(), test_iri_builder());
        let node_iri = test_node_iri();

        let hot_metrics = vec![MetricEntry {
            name: "cpu_temp_celsius".to_string(),
            value: 85.0,
            unit: "celsius".to_string(),
        }];

        // First evaluation fires
        let actions1 = evaluator.evaluate(&node_iri, &hot_metrics).await.unwrap();
        assert!(!actions1.is_empty());

        // Second evaluation while still hot — should produce no actions (dampening)
        let actions2 = evaluator.evaluate(&node_iri, &hot_metrics).await.unwrap();
        assert!(actions2.is_empty(), "Should not re-fire while still alerting");
    }

    #[tokio::test]
    async fn no_actions_when_metrics_are_normal() {
        let evaluator = BuiltInAlertEvaluator::new(builtin_alert_rules(), test_iri_builder());
        let node_iri = test_node_iri();

        let normal_metrics = vec![MetricEntry {
            name: "cpu_temp_celsius".to_string(),
            value: 55.0,
            unit: "celsius".to_string(),
        }];

        let actions = evaluator.evaluate(&node_iri, &normal_metrics).await.unwrap();
        assert!(actions.is_empty());
    }
}
