---
id: FT-002
title: Event & State Model
phase: 1
status: complete
depends-on:
- FT-001
adrs:
- ADR-004
- ADR-005
- ADR-006
- ADR-008
- ADR-031
- ADR-035
- ADR-002
tests:
- TC-013
- TC-014
- TC-015
- TC-016
- TC-017
- TC-018
- TC-019
- TC-020
- TC-021
- TC-025
- TC-026
- TC-027
- TC-090
- TC-091
- TC-092
- TC-103
- TC-104
- TC-105
- TC-106
- TC-210
- TC-219
- TC-223
domains:
- data-model
- consensus
domains-acknowledged: {}
---

The event log is the source of truth for all cluster state. The RDF graph is the continuously maintained read model derived from it. No component writes state directly — all state changes flow through events.

### Event log

The event log is append-only and Raft-replicated across all nodes. Every event has:

```
{
  id:         UUID,
  type:       string,          // e.g. "ResourceReady", "NodeJoined"
  timestamp:  ISO8601,
  source:     resource-path,   // who emitted it
  product:    string | null,   // product scope if applicable
  payload:    {}               // event-specific data
}
```

Events are never modified or deleted. The log is the permanent record of everything that has happened in the cluster.

### RDF projection

The Oxigraph triplestore is populated by projectors — components that consume events from the log and write triples. Each event type has a corresponding projector. Projectors are deterministic: replaying the event log from the beginning always produces the same graph.

The cluster-level graph contains:
- All nodes and their status
- All Products, their versions, and their resource inventories
- All identities and role assignments
- All event subscription relationships between Products
- All ontology declarations and their bindings to Product versions

### Named graphs — operational and analytical planes

Each Product maintains two categories of named graph within Oxigraph:

**Operational graph** (`https://picloud.local/products/{name}/graph`) — the Product's internal RDF state. Live projection of the event log. Schema evolves with the domain. Private: accessible only to workloads within the Product's own IAM scope. Cross-product SPARQL access to the operational graph is rejected at the HTTP layer.

**Data product graphs** (`https://picloud.local/products/{name}/data-products/{dp-name}/graph`) — published, versioned analytical projections. Each `data-product` resource has its own named graph, populated by a SPARQL CONSTRUCT query run against the operational graph on declared trigger events. IAM-gated for consumers. These are the only cross-product readable surfaces in the cluster.

The cluster-level graph contains:
- All nodes and their status
- All Products, their versions, and their resource inventories
- All identities and role assignments
- All event subscription relationships between Products
- All ontology declarations and their bindings to Product versions
- All capabilities, their implementors and consumers
- All data domains, their data products, freshness SLOs and dependency graph

**Platform event stream** — internal cluster events (node joins, resource lifecycle, IAM changes). Available to workloads with platform-level IAM permissions.

**Product event stream** — domain events emitted by a Product's workloads. Declared in the resource definition. Other Products subscribe via `event-subscription` resources. The platform routes events between Products — Products never communicate directly.

### Observability

Because all state is derived from events, the platform provides complete historical observability for free. Any point-in-time cluster state can be reconstructed by replaying the event log to that timestamp. This is not a debugging tool — it is a fundamental property of the architecture.

### Replay

Replay is a first-class platform and product capability. Any product or the platform itself can replay its event log over any time range, against specific aggregates, or in bulk batches of up to 1000 aggregates.

Replay always uses the **currently deployed version's projectors** — not the projectors that originally processed the events. This is how bugs in historical projectors are corrected retroactively. Schema IRIs on every event (ADR-031) ensure current projectors can correctly interpret any historical payload.

Replay builds a **shadow projection** in a separate named graph while the live graph continues serving traffic. When the shadow projection is complete, it is atomically swapped with the live graph. Live traffic is never interrupted.

Replayed events are re-emitted to all active subscribers with a `replay` marker field containing the `replay_id`, `original_timestamp`, and `replayed_at`. Subscribers can inspect this field to suppress irreversible side effects (emails, payments) while still updating their projections. The event `id` field ensures fully idempotent subscribers require no changes.

Replay is accessible via the CLI, the HTTP API, and the SDK. It emits its own lifecycle events (`ReplayStarted`, `ReplayProgress`, `ReplayCompleted`, `ReplayFailed`) which are projected into the cluster RDF graph and subscribable via the standard event stream.

---