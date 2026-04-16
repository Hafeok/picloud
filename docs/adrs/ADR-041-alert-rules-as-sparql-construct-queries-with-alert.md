---
id: ADR-041
title: Alert Rules as SPARQL CONSTRUCT Queries with AlertFired Events
status: accepted
features:
- FT-009
- FT-036
- FT-076
- FT-077
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:b0346aedf1f4d49ee1cbf667d00c2fd8e9e2f2a612d0581fd93e6d2b30e422ca
---

**Status:** Accepted

**Context:** Operators need to know when something is wrong — a node is overheating, a product's error rate is spiking, memory is exhausted. Alerts must be declarative, auditable, and consistent with the platform's event model. Alert lifecycle (fired, resolved) must be observable by any subscriber.

**Decision:** Alerts are produced by SPARQL CONSTRUCT rules (ADR-038) that match `picloud:Alert` typed triples. The inference engine detects when alert triples are asserted or retracted and emits `AlertFired` and `AlertResolved` events respectively. No built-in notification targets — alerts are events, and products built on PiCloud handle delivery.

**Alert triple shape (produced by CONSTRUCT rules):**
```turtle
_:alert a picloud:Alert ;
    picloud:alertType "HighCpuTemperature" ;
    picloud:alertSeverity "critical" ;           // info | warning | critical
    picloud:alertMessage "CPU temperature above 80°C on pi-node-02" ;
    picloud:alertResource <https://picloud.local/nodes/pi-node-02> ;
    picloud:alertTimestamp "2025-07-01T12:00:00Z"^^xsd:dateTime .
```

**Built-in platform alert rules (shipped with the platform):**

| Rule | Threshold | Severity |
|---|---|---|
| High CPU temperature | > 80°C | critical |
| High CPU temperature | > 70°C | warning |
| High memory usage | > 90% | critical |
| High memory usage | > 80% | warning |
| High disk usage | > 90% | critical |
| Node unreachable | Raft heartbeat missed | critical |
| Product workload failed | `ResourceStatus = Failed` | critical |

**Custom alert rules** are declared as `inference-rule` resources in product or platform `.picloud` files. Any CONSTRUCT query that produces `picloud:Alert` triples is an alert rule.

**Example — product request error rate alert:**
```bicep
inference-rule 'high-error-rate' = {
  scope: 'photo-app'
  trigger: 'event'
  trigger-events: ['MetricRecorded']
  construct: '''
    CONSTRUCT {
      ?product a picloud:Alert ;
               picloud:alertType "HighErrorRate" ;
               picloud:alertSeverity "warning" ;
               picloud:alertMessage "Error rate above 5%" ;
               picloud:alertResource ?product .
    }
    WHERE {
      ?product a picloud:Product ;
               picloud:errorRatePercent ?rate .
      FILTER(?rate > 5.0)
    }
  '''
}
```

**AlertFired event shape:**
```json
{
  "type": "AlertFired",
  "payload": {
    "alert_type": "HighCpuTemperature",
    "severity": "critical",
    "message": "CPU temperature above 80°C on pi-node-02",
    "resource_iri": "https://picloud.local/nodes/pi-node-02",
    "rule_iri": "https://picloud.local/inference-rules/high-cpu-temp-critical",
    "fired_at": "2025-07-01T12:00:00Z"
  }
}
```

**AlertResolved event** is emitted when the alert triple is retracted — i.e. the CONSTRUCT query no longer matches. Resolution is automatic and event-driven.

**Querying active alerts:**
```sparql
SELECT ?resource ?type ?severity ?message ?timestamp
WHERE {
  ?alert a picloud:Alert ;
         picloud:alertResource ?resource ;
         picloud:alertType ?type ;
         picloud:alertSeverity ?severity ;
         picloud:alertMessage ?message ;
         picloud:alertTimestamp ?timestamp .
}
ORDER BY DESC(?timestamp)
```

**Rationale:**
- Alert rules are resources — versioned, auditable, deployed via `picloud resource apply`
- `AlertFired` and `AlertResolved` as events means any product can subscribe and build notification, escalation, or auto-remediation workflows
- No built-in notification targets — consistent with the platform's composability philosophy (ADR-018). A notification product built on PiCloud handles Slack, email, PagerDuty etc.
- Active alerts are queryable from the RDF graph at any time — `picloud graph query` gives the current alert state
- Alert resolution is automatic — when the condition clears, the event fires. No manual acknowledgement needed (though products can implement that on top)

**Rejected alternatives:**
- **External alerting (Alertmanager, PagerDuty rules)** — requires external infrastructure and a separate rule language when SPARQL already queries the full platform state.
- **Threshold-only alerting** — simple thresholds cannot express complex conditions that span multiple resource types, which SPARQL handles naturally.

**Consequences:**
- The inference engine must efficiently diff produced triples between evaluations to detect assertions and retractions
- Alert storms (rapid fire/resolve cycles) should be dampened — a minimum 60-second hold-off before re-firing the same alert on the same resource
- Built-in platform alert rules are shipped as `.ttl` files in the platform binary and loaded at startup
- `picloud:Alert` becomes a well-known class in the platform ontology — documented in the SDK