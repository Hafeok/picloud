---
id: TC-125
title: metrics_upsert
type: scenario
status: passing
validates:
  features:
  - FT-009
  - FT-076
  adrs:
  - ADR-040
phase: 1
runner: cargo-test
runner-args: metrics_upsert
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 1.0s
---

wait for two consecutive `MetricRecorded` events from the same node. Assert the graph holds only the latest metric values (not a growing list of historical values).