---
id: TC-149
title: parquet_write_read
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-046
phase: 1
runner: cargo-test
runner-args: parquet_write_read
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 0.8s
---

ingest 1,000 OTel spans via the OTLP endpoint. Wait for Parquet flush. Run a DataFusion SQL query: `SELECT COUNT(*) FROM traces WHERE service_name = 'test-service'`. Assert count = 1000.