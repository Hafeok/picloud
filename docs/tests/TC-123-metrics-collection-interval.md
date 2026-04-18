---
id: TC-123
title: metrics_collection_interval
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
runner-args: metrics_collection_interval
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 3.9s
---

start a node. Wait 30 seconds. Assert at least 2 `MetricRecorded` events in the log for that node, each containing CPU usage, memory usage, disk usage, and CPU temperature.