---
id: TC-261
title: W3C traceparent header propagated from platform to workload
type: scenario
status: passing
runner: cargo-test
runner-args: "tc261_w3c_traceparent_header_propagated_from_platform_to_workload"
validates:
  features: [FT-048]
  adrs: []
phase: 2
last-run: 2026-04-15T12:09:45.148421724+00:00
last-run-duration: 1.4s
---

## Description

Verifies that the PiCloud platform generates a valid W3C traceparent header
and injects it as the `TRACEPARENT` environment variable into workloads at
schedule time.

### What is tested

1. **Binary workloads** receive a `TRACEPARENT` env var when spawned
2. The generated traceparent conforms to the W3C Trace Context format:
   `{version}-{trace-id}-{parent-id}-{trace-flags}` (e.g. `00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01`)
3. The `is_valid_traceparent` helper correctly validates well-formed and malformed values
4. **Container workloads** (simulated) receive the TRACEPARENT injection
5. Each workload invocation receives a **unique** traceparent (trace-ids never reuse)
6. Platform-injected TRACEPARENT **overrides** any user-supplied value in the workload env
7. The TRACEPARENT format is validated inside the actual spawned process (end-to-end)