---
id: TC-318
title: Trace propagation exit — traceparent header flows to workloads
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc318_trace_propagation_exit_traceparent_header_flows_to_workloads"
validates:
  features: [FT-048]
  adrs: []
phase: 2
last-run: 2026-04-15T12:09:45.148421724+00:00
last-run-duration: 1.6s
---

## Description

Exit criterion for FT-048: W3C trace context propagation from platform to workloads.

### Gates

1. **Binary workload** receives a valid W3C `TRACEPARENT` env var (version `00`, 32-char trace-id, 16-char parent-id)
2. **Container workload** (simulated) receives the TRACEPARENT injection
3. **Multiple workloads** receive unique trace-ids and parent-ids (no reuse)
4. **All workloads** receive a platform-generated TRACEPARENT, even when no trace context was supplied by the caller
5. **EventEnvelope** supports an optional `traceparent` field that survives serialization round-trips and is omitted from JSON when `None`

### Implementation summary

- `ProcessScheduler::generate_traceparent()` generates W3C-compliant traceparent strings
- `ProcessScheduler::is_valid_traceparent()` validates traceparent format
- Binary workloads: `TRACEPARENT` injected via `Command::env()` after OTEL vars
- Container workloads (podman/docker): `TRACEPARENT` injected via `-e` flag
- Container workloads (youki): `TRACEPARENT` added to OCI bundle env vars
- Reverse proxy: propagates existing `traceparent` headers, generates new ones when missing
- HTTP handlers: extract `traceparent` from request headers into `EventEnvelope.traceparent`