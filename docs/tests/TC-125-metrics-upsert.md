---
id: TC-125
title: metrics_upsert
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-040
phase: 1
---

wait for two consecutive `MetricRecorded` events from the same node. Assert the graph holds only the latest metric values (not a growing list of historical values).