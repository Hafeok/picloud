---
id: TC-146
title: otlp_trace_ingestion
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-045
phase: 1
runner: cargo-test
runner-args: "otlp_trace_ingestion"
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 1.4s
---

POST a valid OTLP trace payload to `https://picloud.local/otel/v1/traces`. Assert 200. Assert the trace appears in the Parquet time-series store within 30 seconds (verified via DataFusion query).