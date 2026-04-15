---
id: TC-338
title: Scoped replay exit — single aggregate replay completes
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc338_scoped_replay_exit_single_aggregate_replay_completes"
validates:
  features: [FT-082]
  adrs: [ADR-035]
phase: 3
last-run: 2026-04-15T17:17:52.849380718+00:00
last-run-duration: 0.7s
---

## Description

Exit-criteria gate for aggregate-scoped replay. Given an aggregate-scoped replay request
targeting a single aggregate, the replay operation MUST:

a) Filter events to only the target aggregate (sku-001 out of sku-001 + sku-002)
b) Build a shadow projection containing only the filtered events
c) Atomically swap shadow into the target graph (clearing stale data)
d) Record aggregate scope metadata in the shadow graph via start_replay
   (replayAggregateType and replayAggregateId triples)
e) Report correct (filtered) event count (3 sku-001 events, not 5 total)
f) Clean up the shadow graph after swap (zero residual triples)
g) Leave the default (live) graph unmodified