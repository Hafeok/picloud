---
id: TC-256
title: OTel event stream delivers traces to in-process subscriber
type: scenario
status: passing
runner: cargo-test
runner-args: "tc256_otel_event_stream_delivers_traces_to_in_process_subscriber"
validates:
  features: [FT-043]
  adrs: [ADR-045]
phase: 2
last-run: 2026-04-15T11:05:06.898660060+00:00
last-run-duration: 0.4s
---

## Description

Verifies that the OtelStream in-process pub/sub channel delivers traces, metrics,
and logs to subscribers correctly.

### Steps

1. Create an OtelStream and subscribe to it
2. Publish a span — verify the subscriber receives it with correct trace_id and fields
3. Publish a metric — verify the subscriber receives it with correct name and value
4. Publish a log — verify the subscriber receives it with correct body and severity
5. Subscribe multiple receivers and verify each gets a copy of every published datum
6. Batch-publish spans via `publish_spans` — verify all arrive in order
7. Publish with no subscribers — verify no panic (data is silently dropped)