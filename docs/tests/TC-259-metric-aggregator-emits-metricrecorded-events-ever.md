---
id: TC-259
title: Metric aggregator emits MetricRecorded events every 15 seconds
type: scenario
status: passing
runner: cargo-test
runner-args: "tc259_metric_aggregator_emits_metricrecorded_events_every_15_seconds"
validates:
  features: [FT-046]
  adrs: [ADR-045]
phase: 2
last-run: 2026-04-15T11:46:39.275799803+00:00
last-run-duration: 4.2s
---

## Description

Verifies that the OtelAggregator subscribes to the OtelStream, aggregates OTel
metric data points by name (computing the mean value per metric), and emits
MetricRecorded events to the platform event log at a regular interval (default 15s).

Steps:
1. Create an OtelStream, event log, and OtelAggregator with a short test interval
2. Publish several OTel metric data points to the stream
3. Wait for the first aggregation interval to fire
4. Verify a MetricRecorded event was emitted to the event log
5. Verify the event payload contains correct aggregated metric values (mean per name)
6. Publish more metrics and verify a second MetricRecorded event arrives
7. Verify that when no metrics are published, no MetricRecorded event is emitted