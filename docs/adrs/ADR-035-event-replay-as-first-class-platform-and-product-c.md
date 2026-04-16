---
id: ADR-035
title: Event Replay as First-Class Platform and Product Capability
status: accepted
features:
- FT-002
- FT-081
- FT-082
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:33525a5cae02a60e9118c7b9ce06b408e3fb01e0da4c53eae290a89ce51f4cfa
---

**Status:** Accepted

**Context:** The event log is append-only, permanent, and Raft-replicated. Every state change — platform and product — flows through it. This means the RDF graph (the read model) can always be rebuilt by re-running projectors against the log. However, replay is only useful if it is a deliberate, observable, controllable operation — not a recovery mechanism that operators have to engineer themselves.

Two categories of failure motivate this:
- **Projector bugs** — a bug in a projector writes incorrect triples into the RDF graph. The fix is deployed in a new Product version. The corrected projector must be run against historical events to repair the graph.
- **Subscriber inconsistency** — a downstream Product's projection has drifted because events were missed or misprocessed. Replaying the source Product's events from a known-good point re-establishes consistency.

**Decision:** Replay is a first-class capability available to both the platform itself and to every Product. It is accessible via the CLI and the SDK/HTTP API.

### Replay model

Replay reads events from the log between a `from` and optional `to` timestamp, re-runs them through the **currently deployed version's projectors**, and re-emits them to all active event subscribers. Replay always uses the current projectors — never the projectors from the version that originally emitted the events. This is the mechanism by which bugs in previous projector versions are corrected (see ADR-031 — schema IRIs ensure current projectors can interpret any historical event payload).

### Replay scope

Three scopes are supported:

**Platform replay** — replays the platform event log. Rebuilds the cluster-level RDF graph. Used when platform projector bugs corrupt cluster state.

```bash
picloud cluster replay --from "2025-06-01T00:00:00Z"
picloud cluster replay --from "2025-06-01T00:00:00Z" --to "2025-06-02T00:00:00Z"
```

**Product replay** — replays all events in a Product's event store. Rebuilds the Product's RDF graph.

```bash
picloud resource replay photo-app --from "2025-06-01T00:00:00Z"
```

**Aggregate replay** — replays one or more specific aggregates. Supports a single aggregate, a list, or a batch of up to N aggregates.

```bash
# Single aggregate
picloud resource replay photo-app \
  --aggregate Photo --id 123e4567-e89b-12d3-a456-426614174000 \
  --from "2025-06-01T00:00:00Z"

# Batch — up to 1000 aggregate IDs from a file
picloud resource replay photo-app \
  --aggregate Photo --ids-file ./photo-ids.txt \
  --from "2025-06-01T00:00:00Z"
```

### Replay always serves live traffic

The platform continues serving the current RDF graph during replay. The new projection is built in a **shadow graph** — a separate named graph in Oxigraph scoped to the replay operation. When the shadow projection reaches the `to` timestamp (or the present if no `to` is given), it is validated and atomically swapped with the live graph. The swap is itself an event in the log.

This is consistent with the atomic cutover model for Product upgrades (ADR-021) — state transitions are always atomic, never partial.

### Marked replay events

Replayed events are distinguishable from live events. Every replayed event envelope carries two additional fields:

```json
{
  "id": "uuid",
  "schema": "https://picloud.local/schemas/events/PhotoCreated/v1",
  "type": "PhotoCreated",
  "timestamp": "2025-06-01T10:00:00Z",
  "replay": {
    "is_replay": true,
    "replay_id": "uuid",
    "original_timestamp": "2025-06-01T10:00:00Z",
    "replayed_at": "2025-07-01T09:00:00Z"
  },
  ...
}
```

`replay_id` groups all events from a single replay operation. `original_timestamp` is when the event was first written. `replayed_at` is when it was re-emitted.

Subscribers receive replayed events on the same channels as live events. The `replay` field allows subscribers to make explicit decisions — for example, skipping email sends or payment charges on replay while still updating their RDF projections. Subscribers that are fully idempotent via the event `id` field require no changes — the platform deduplicates automatically.

**Platform contract:** all event subscribers should be idempotent by default. The `replay` field is additional information, not a crutch for non-idempotent implementations.

### Replay via SDK and HTTP API

Replay is available programmatically so Products can trigger self-healing workflows:

```
POST https://picloud.local/products/photo-app/event-store/photos/replay
{
  "from": "2025-06-01T00:00:00Z",
  "aggregate_type": "Photo",
  "aggregate_ids": ["uuid-1", "uuid-2", ...],  // omit for full store replay
  "to": "2025-06-02T00:00:00Z"                 // omit for replay to present
}
```

Returns a `replay_id`. The replay operation emits a `ReplayStarted` event and a `ReplayCompleted` or `ReplayFailed` terminal event — subscribable via the standard event stream.

### Replay lifecycle events

```
ReplayRequested   — operator or API triggered a replay
ReplayStarted     — shadow projection is building
ReplayProgress    — periodic progress (events processed / total)
ReplayCompleted   — shadow graph swapped with live graph
ReplayFailed      — replay aborted, live graph unchanged, reason attached
```

All replay events are written to the platform log and projected into the cluster RDF graph. A replay operation is fully auditable.

**Rationale:**
- Replay is the correctness guarantee of event sourcing — without it, a projector bug is permanent damage rather than a recoverable state
- Shadow projection with atomic swap means replay never degrades live service
- Marked replay events give subscribers the information to make correct decisions without mandating a specific behaviour
- Using current projectors against historical events (via schema IRIs) is the mechanism by which bugs are fixed retroactively — this is the core value of the ADR-031 schema versioning decision
- Batch aggregate replay (up to 1000) covers the common operational case of targeted repair without requiring a full store replay
- CLI, HTTP API, and SDK access means replay can be scripted, automated, or triggered by monitoring systems

**Rejected alternatives:**
- **Manual replay scripts** — operators would need to write custom replay logic, with no shadow graph protection and no auditable lifecycle.
- **Snapshot-based recovery only** — snapshots capture state at a point in time but cannot fix projector bugs retroactively, which is the primary use case for replay.

**Consequences:**
- The shadow graph mechanism requires Oxigraph to support multiple named graphs simultaneously — it does (ADR-006)
- Batch replay of 1000 aggregates is a resource-intensive operation — the platform should enforce concurrency limits (one active replay per Product at a time)
- `ReplayProgress` events should be emitted frequently enough to be useful but not so frequently that they flood the event log — every 100 events processed is a reasonable default
- Subscribers that perform irreversible side effects (email, payment, external API calls) must inspect the `replay.is_replay` field — this should be documented prominently in the SDK