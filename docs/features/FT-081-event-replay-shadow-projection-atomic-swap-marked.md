---
id: FT-081
title: Event replay — shadow projection, atomic swap, marked replay events
phase: 3
status: complete
depends-on: []
adrs:
- ADR-035
- ADR-031
tests:
- TC-280
- TC-337
domains: []
domains-acknowledged: {}
---

## Description

Event replay is a first-class capability for both the platform and Products (ADR-035). Replay reads events from the log, re-runs them through the **currently deployed** projectors, and rebuilds the RDF graph in a shadow projection that is atomically swapped with the live graph.

### Replay scopes

- **Platform replay** — replays the platform event log, rebuilds the cluster-level RDF graph
- **Product replay** — replays all events in a Product's event store, rebuilds the Product's RDF graph

```bash
picloud cluster replay --from "2025-06-01T00:00:00Z"
picloud resource replay photo-app --from "2025-06-01T00:00:00Z"
```

### Shadow projection and atomic swap

1. Replay creates a shadow named graph in Oxigraph
2. Events are replayed through current projectors into the shadow graph
3. The live graph continues serving traffic during replay
4. When replay reaches the target offset, the shadow graph atomically replaces the live graph
5. No query ever sees partial replay state

### Marked replay events

Replayed events carry a `replay` field:
```json
{
  "replay": {
    "is_replay": true,
    "replay_id": "uuid",
    "original_timestamp": "2025-06-01T10:00:00Z",
    "replayed_at": "2025-07-01T09:00:00Z"
  }
}
```

Subscribers can inspect `is_replay` to suppress irreversible side effects (emails, payments) while still updating projections.

### Why current projectors

Replay always uses the currently deployed version's projectors — never the projectors that originally processed the events. This is how projector bugs are fixed retroactively. Schema IRIs (ADR-031) on every event ensure current projectors can interpret any historical payload.

### Lifecycle events

`ReplayStarted`, `ReplayProgress`, `ReplayCompleted`, `ReplayFailed` — all projected into the cluster graph.
