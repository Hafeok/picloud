---
id: TC-313
title: OTel stream exit — traces delivered to in-process subscriber
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc313_otel_stream_exit_traces_delivered_to_in_process_subscriber"
validates:
  features: [FT-043]
  adrs: [ADR-045]
phase: 2
last-run: 2026-04-15T11:05:06.898660060+00:00
last-run-duration: 0.5s
---

## Description

End-to-end exit-criteria test verifying that OTLP traces posted via the HTTP
endpoint at `/otel` are delivered to in-process OtelStream subscribers.

This validates the full data path: HTTP POST /otel → parse_otlp_json → OtelStream.publish → subscriber.recv.

### Steps

1. Build the HTTP server with a shared OtelStream reference
2. Subscribe to the OtelStream before posting any data
3. POST OTLP traces (resourceSpans format) via /otel — verify HTTP 200 with accepted count
4. Receive the published spans from the in-process subscriber
5. Verify trace_id, span_id, parent_span_id, service_name, and operation_name match
6. Verify computed duration_ms from start/end timestamps
7. POST metrics via /otel — verify the subscriber also receives them with correct fields
8. POST a mixed payload (spans + metrics + logs) — verify all three types arrive at the subscriber