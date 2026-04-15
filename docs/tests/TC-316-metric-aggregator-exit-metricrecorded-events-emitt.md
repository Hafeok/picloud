---
id: TC-316
title: Metric aggregator exit — MetricRecorded events emitted on schedule
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc316_metric_aggregator_exit_metricrecorded_events_emitted_on_schedule"
validates:
  features: [FT-046]
  adrs: [ADR-045]
phase: 2
last-run: 2026-04-15T11:46:39.275799803+00:00
last-run-duration: 2.7s
---

## Description

End-to-end exit-criteria verification that OTel metric data flowing through the
OtelStream is aggregated into MetricRecorded platform events on the expected schedule.

Validates the full data path:
  OtelStream.publish(Metric) -> collector task -> metric buffer ->
  aggregator task -> EventLog.append(MetricRecorded)

Steps:
1. Create the full aggregation pipeline (OtelStream + OtelAggregator + EventLog)
2. Publish diverse OTel metrics over two aggregation windows
3. Verify MetricRecorded events appear on schedule (one per interval)
4. Verify each event contains the correct time-windowed aggregates
5. Verify TelemetryAggregated events are also emitted (co-existence)
6. Verify MetricRecorded event schema IRI is correctly formed