---
id: TC-106
title: aggregate_replay
type: scenario
status: failing
validates:
  features:
  - FT-002
  adrs:
  - ADR-035
phase: 1
runner: picloud-test
runner-args: "aggregate-replay"
---

replay a single aggregate (Photo ID `abc123`) from a product event store. Assert only that aggregate's events are re-emitted. Assert other aggregates are unaffected.