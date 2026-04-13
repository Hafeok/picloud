---
id: TC-149
title: parquet_write_read
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-046
phase: 1
runner: picloud-test
runner-args: "parquet-write-read"
---

ingest 1,000 OTel spans via the OTLP endpoint. Wait for Parquet flush. Run a DataFusion SQL query: `SELECT COUNT(*) FROM traces WHERE service_name = 'test-service'`. Assert count = 1000.