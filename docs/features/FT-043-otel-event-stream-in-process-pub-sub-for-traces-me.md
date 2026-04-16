---
id: FT-043
title: OTel event stream — in-process pub/sub for traces, metrics, logs
phase: 2
status: complete
depends-on: []
adrs:
- ADR-045
- ADR-046
tests:
- TC-256
- TC-313
domains: []
domains-acknowledged: {}
---

## Description

Provides an in-process pub/sub channel (OtelStream) for OpenTelemetry data
flowing through the PiCloud platform. The stream uses a tokio broadcast channel
with a bounded buffer to deliver spans, metrics, and logs to multiple subscribers.

Key capabilities:
- **Publish/subscribe**: Any component can subscribe to the OtelStream and receive
  all future OtelDatum items (spans, metrics, logs) via a broadcast receiver.
- **Multiple subscribers**: Each subscriber gets an independent copy of every datum,
  enabling parallel consumers (TelemetryStore, OtelAggregator, future exporters).
- **Batch publish**: `publish_spans` and `publish_metrics` methods for efficient
  bulk ingestion from the OTLP endpoint.
- **Graceful degradation**: Publishing with no subscribers silently drops data —
  the stream never blocks or errors on send.

The OtelStream is the central fan-out point between the OTLP HTTP endpoint (FT-042)
and downstream consumers like the JsonlTelemetryStore (ADR-046) and OtelAggregator
which emits TelemetryAggregated events to the platform event log.
