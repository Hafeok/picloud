---
id: FT-042
title: OTLP endpoint at picloud.local/otel
phase: 2
status: complete
depends-on: []
adrs: []
tests:
- TC-255
- TC-312
domains: []
domains-acknowledged: {}
---

## Description

Provides an OTLP (OpenTelemetry Protocol) endpoint at `https://picloud.local/otel` that accepts
traces and metrics via HTTP POST. Workloads export telemetry to this endpoint using standard
OTLP JSON format (resourceSpans/scopeSpans) or a simplified format. Ingested data is published
to the in-process OTel stream and written to the JSONL telemetry store for querying via
`/telemetry/spans` and `/telemetry/metrics`.
