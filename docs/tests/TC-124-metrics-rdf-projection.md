---
id: TC-124
title: metrics_rdf_projection
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-040
phase: 1
runner: picloud-test
runner-args: "metrics-rdf-projection"
---

after a `MetricRecorded` event, query the node IRI via SPARQL. Assert `picloud:cpuUsagePercent`, `picloud:memoryUsedMb`, `picloud:memoryTotalMb`, `picloud:cpuTempCelsius`, and `picloud:metricsUpdatedAt` are present.