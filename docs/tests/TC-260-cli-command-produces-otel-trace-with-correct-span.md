---
id: TC-260
title: CLI command produces OTel trace with correct span hierarchy
type: scenario
status: passing
runner: cargo-test
runner-args: "tc260_cli_command_produces_otel_trace_with_correct_span_hierarchy"
validates:
  features: [FT-047]
  adrs: [ADR-045]
phase: 2
last-run: 2026-04-15T11:57:35.263079053+00:00
last-run-duration: 0.5s
---

## Description

Verifies that every CLI command invocation produces a well-formed OTel trace:

1. A root span with a valid trace_id (32 hex chars), span_id (16 hex chars), no parent, and `service_name = "picloud-cli"`
2. Child spans for HTTP calls that reference the root span as their parent
3. All spans in a command share the same trace_id
4. The OTLP JSON payload is well-formed with all required fields
5. The W3C traceparent header follows the `00-{trace_id}-{span_id}-01` format
6. Span timing: end_time >= start_time for all spans