---
id: TC-255
title: OTLP endpoint accepts traces and metrics at picloud.local/otel
type: scenario
status: passing
runner: cargo-test
runner-args: "tc255_otlp_endpoint_accepts_traces_and_metrics_at_picloud_local_otel"
validates:
  features: [FT-042]
  adrs: [ADR-045]
phase: 2
last-run: 2026-04-15T11:00:00.208720960+00:00
last-run-duration: 0.4s
---

## Description

Verifies that the OTLP endpoint at `/otel` accepts both traces and metrics via POST.

Steps:
1. POST a standard OTLP resourceSpans payload to /otel — expect 200 with accepted count of 2
2. POST a simplified metrics payload to /otel — expect 200 with accepted count of 2
3. POST a mixed payload (traces + metrics) — expect 200 with total accepted count of 2
4. POST an empty payload — expect 200 with accepted: 0
5. Verify the endpoint handles the standard OTLP format (resourceSpans/scopeSpans)