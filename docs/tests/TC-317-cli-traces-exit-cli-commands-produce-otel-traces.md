---
id: TC-317
title: CLI traces exit — CLI commands produce OTel traces
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc317_cli_traces_exit_cli_commands_produce_otel_traces"
validates:
  features: [FT-047]
  adrs: [ADR-045]
phase: 2
last-run: 2026-04-15T11:57:35.263079053+00:00
last-run-duration: 0.5s
---

## Description

Exit criterion: every CLI command type produces a valid OTel trace.

Verifies that all 23 CLI subcommand categories (cluster, resource, identity, events, graph, ca, sdk, tag, alerts, telemetry, volume, compile, new, image, registry, capability, data-domain, data-product, etc.) each produce:

1. A root span with no parent, valid IDs, and correct service_name
2. At least one child span for the HTTP call, correctly parented
3. A well-formed OTLP JSON payload
4. Unique trace_ids per invocation (no collisions across commands)