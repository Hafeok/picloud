---
id: TC-146
title: otlp_trace_ingestion
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-045
phase: 1
runner: cargo-test
runner-args: "otlp_trace_ingestion"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.9s
failure-message: "No matching test function found (0 tests ran)"
---

POST a valid OTLP trace payload to `https://picloud.local/otel/v1/traces`. Assert 200. Assert the trace appears in the Parquet time-series store within 30 seconds (verified via DataFusion query).