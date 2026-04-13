---
id: TC-146
title: otlp_trace_ingestion
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-045
phase: 1
---

POST a valid OTLP trace payload to `https://picloud.local/otel/v1/traces`. Assert 200. Assert the trace appears in the Parquet time-series store within 30 seconds (verified via DataFusion query).