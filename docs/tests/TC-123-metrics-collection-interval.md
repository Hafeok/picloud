---
id: TC-123
title: metrics_collection_interval
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-040
phase: 1
---

start a node. Wait 30 seconds. Assert at least 2 `MetricRecorded` events in the log for that node, each containing CPU usage, memory usage, disk usage, and CPU temperature.