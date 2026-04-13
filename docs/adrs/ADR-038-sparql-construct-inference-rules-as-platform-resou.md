---
id: ADR-038
title: SPARQL CONSTRUCT Inference Rules as Platform Resources
status: accepted
features: [FT-009, FT-057, FT-058]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** PiCloud needs a mechanism to derive new knowledge from existing graph state — for IAM group membership (ADR-037), for operational alerts, and for any future rule-based automation. The mechanism must be declarative, auditable, version-controlled, and consistent with the platform's resource model.

**Decision:** SPARQL CONSTRUCT queries are a first-class resource type (`inference-rule`). Rules are declared in `.picloud` files, deployed with the platform or a product, stored in the RDF graph, and evaluated by the platform's inference engine. Rules produce triples that are written back to the graph. When produced triples are new (assertions) or removed (retractions), the engine emits events.

**Rule resource:**
```bicep
inference-rule 'high-memory-alert' = {
  description: 'Alert when node memory exceeds 85%'
  scope: 'platform'                    // 'platform' or product name
  trigger: 'event'                     // evaluate on matching events
  trigger-events: ['MetricRecorded']   // which events trigger evaluation
  reconciliation: true                 // also run on 10-minute schedule
  construct: '''
    CONSTRUCT {
      ?node a picloud:Alert ;
            picloud:alertType "HighMemoryUsage" ;
            picloud:alertSeverity "warning" ;
            picloud:alertMessage "Node memory above 85%" ;
            picloud:alertResource ?node ;
            picloud:alertTimestamp ?now .
    }
    WHERE {
      ?node a picloud:Node ;
            picloud:memoryUsedMb ?used ;
            picloud:memoryTotalMb ?total .
      BIND(NOW() AS ?now)
      FILTER(?used / ?total > 0.85)
    }
  '''
}
```

**Evaluation model:**
1. A triggering event arrives (e.g. `MetricRecorded`)
2. The engine identifies all rules triggered by that event type
3. Each rule's CONSTRUCT query runs against the current graph
4. New triples are asserted — retracted triples are removed
5. For each **new** `picloud:Alert` triple: an `AlertFired` event is emitted
6. For each **removed** `picloud:Alert` triple: an `AlertResolved` event is emitted
7. For each **new** `picloud:hasMember` triple: a `GroupMembershipChanged` event is emitted
8. All produced triples are projected into the appropriate named graph

**Reconciliation pass:**
Every 10 minutes, all rules with `reconciliation: true` are evaluated regardless of events. This catches any drift — for example, a rule that should have fired but did not because an event was missed during a node restart. The reconciliation pass is itself an event (`ReconciliationCompleted`) in the platform log.

**Rule scoping:**
- `scope: 'platform'` — rule runs against the cluster-level graph, available to platform operators only
- `scope: 'photo-app'` — rule runs against the product's named graph, available to the product's IAM scope

**Rationale:**
- SPARQL CONSTRUCT is the natural fit — rules are graph pattern queries that produce graph facts
- Rules are resources — versioned, auditable, IRI-addressable, deployed via `picloud resource apply`
- Event-driven evaluation gives fast cascading effects across the cluster
- 10-minute reconciliation is the safety net — eventual consistency with a bounded staleness window
- Scoping means products can define their own inference rules without platform operator involvement
- Alert lifecycle (fired/resolved) as events means any product can subscribe and build notification workflows

**Rejected alternatives:**
- **Hardcoded inference logic** — new inference patterns would require platform code changes and releases rather than declarative resource definitions.
- **External rules engine (Drools, OPA)** — adds an external dependency with its own data model when the platform already has SPARQL and RDF as native capabilities.

**Consequences:**
- The inference engine needs to track which triples were produced by which rule to detect retractions
- Rule evaluation must be idempotent — running the same rule twice produces the same triples
- Expensive CONSTRUCT queries on large graphs must be bounded — rule authors should use graph scoping and LIMIT where appropriate
- One active reconciliation pass at a time — concurrent passes are not permitted