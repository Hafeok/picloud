---
id: TC-312
title: OTLP endpoint exit — traces and metrics accepted at /otel
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc312_otlp_endpoint_exit_traces_and_metrics_accepted_at_otel"
validates:
  features: [FT-042]
  adrs: [ADR-045]
phase: 2
last-run: 2026-04-15T11:00:00.208720960+00:00
last-run-duration: 0.3s
---

## Description

Exit-criteria test for the OTLP endpoint. Validates end-to-end trace and metric ingestion
via `/otel` with verification that data is queryable via the telemetry query endpoints.

Steps:
1. POST OTLP traces (resourceSpans format) to /otel — verify accepted
2. POST OTLP metrics to /otel — verify accepted
3. Query /telemetry/spans — verify the ingested spans are returned with correct service_name
4. Query /telemetry/metrics — verify the ingested metrics are returned with correct name
5. Verify service_name filtering works on /telemetry/spans query