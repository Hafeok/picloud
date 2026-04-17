---
id: FT-057
title: inference-rule resource type — SPARQL CONSTRUCT, event-triggered + 10min reconciliation
phase: 3
status: planned
depends-on: []
adrs:
- ADR-038
- ADR-006
tests:
- TC-228
domains: []
domains-acknowledged: {}
---

## Description

SPARQL CONSTRUCT queries are a first-class resource type (`inference-rule`) (ADR-038). Rules are declared in `.picloud` files, deployed with the platform or a product, and evaluated by the platform's inference engine. Rules produce triples that are written back to the graph.

### Resource syntax

```bicep
inference-rule 'high-memory-alert' = {
  description: 'Alert when node memory exceeds 85%'
  scope: 'platform'
  trigger: 'event'
  trigger-events: ['MetricRecorded']
  reconciliation: true
  construct: '''
    CONSTRUCT {
      ?node a picloud:Alert ;
            picloud:alertType "HighMemoryUsage" ;
            picloud:alertSeverity "warning" ;
            picloud:alertResource ?node .
    }
    WHERE {
      ?node a picloud:Node ;
            picloud:memoryUsedMb ?used ;
            picloud:memoryTotalMb ?total .
      FILTER(?used / ?total > 0.85)
    }
  '''
}
```

### Evaluation model

1. A triggering event arrives (e.g., `MetricRecorded`)
2. The engine identifies all rules triggered by that event type
3. Each rule's CONSTRUCT query runs against the current graph
4. New triples are asserted; retracted triples are removed
5. For new `picloud:Alert` triples → `AlertFired` event
6. For removed `picloud:Alert` triples → `AlertResolved` event
7. For new `picloud:hasMember` triples → `GroupMembershipChanged` event

### Reconciliation

Every 10 minutes, all rules with `reconciliation: true` are evaluated regardless of events. This catches drift — for example, a rule that should have fired but did not because an event was missed during a node restart. `ReconciliationCompleted` is emitted after each pass.

### Scoping

- `scope: 'platform'` — rule runs against the cluster-level graph
- `scope: 'photo-app'` — rule runs against the product's named graph

### Properties

- Rules are idempotent — running the same rule twice produces the same triples
- The engine tracks which triples were produced by which rule to detect retractions
- One active reconciliation pass at a time
- Rules are resources — versioned, auditable, IRI-addressable, deployed via `picloud resource apply`
