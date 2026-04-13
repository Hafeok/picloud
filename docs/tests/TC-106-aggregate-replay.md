---
id: TC-106
title: aggregate_replay
type: scenario
status: passing
validates:
  features:
  - FT-002
  adrs:
  - ADR-035
phase: 1
runner: cargo-test
runner-args: "tc106_aggregate_replay"
---

replay a single aggregate (Photo ID `abc123`) from a product event store. Assert only that aggregate's events are re-emitted. Assert other aggregates are unaffected.