---
id: TC-281
title: Aggregate-scoped replay replays events for single aggregate
type: scenario
status: passing
runner: cargo-test
runner-args: "tc281_aggregate_scoped_replay_replays_events_for_single_aggregate"
validates:
  features: [FT-082]
  adrs: [ADR-035]
phase: 3
last-run: 2026-04-17T10:16:54.149119908+00:00
last-run-duration: 0.7s
---

## Description

Verifies that aggregate-scoped replay correctly filters and replays events for a single aggregate.

Given events for multiple aggregates (Order-001, Order-002, Invoice-001), when a replay is
requested for only Order-001, then:

1. Only the 3 Order-001 events are replayed into the shadow graph
2. Order-002 and Invoice events are excluded from the replay
3. The shadow graph is atomically swapped into the target graph
4. The shadow graph is cleaned up after swap
5. The default (live) graph is not modified
6. Batch replay with multiple aggregate IDs works correctly (Order-001 + Order-002 = 5 events)
7. Batch replay with > 1000 aggregate IDs is rejected with a validation error
8. Type-only filter (no specific IDs) replays all events matching the aggregate type