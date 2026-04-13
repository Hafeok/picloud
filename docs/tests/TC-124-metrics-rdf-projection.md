---
id: TC-124
title: metrics_rdf_projection
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-040
phase: 1
runner: cargo-test
runner-args: "metrics_rdf_projection"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

after a `MetricRecorded` event, query the node IRI via SPARQL. Assert `picloud:cpuUsagePercent`, `picloud:memoryUsedMb`, `picloud:memoryTotalMb`, `picloud:cpuTempCelsius`, and `picloud:metricsUpdatedAt` are present.