---
id: TC-257
title: Parquet time-series store writes hourly partitioned trace files
type: scenario
status: passing
runner: cargo-test
runner-args: "tc257_parquet_time_series_store_writes_hourly_partitioned_trace_files"
validates:
  features: [FT-044]
  adrs: [ADR-046]
phase: 2
last-run: 2026-04-18T14:42:31.417515579+00:00
last-run-duration: 0.7s
---

## Description

Verifies that the ParquetTelemetryStore writes trace spans and metrics as valid
Apache Parquet files organized into hourly partition directories (`traces/{YYYY-MM-DDTHH}/`).
Tests the full write-read cycle including partition creation, Parquet magic byte validation,
data integrity on round-trip, multi-hour partitioning, and filtering by service_name,
operation_name, and min_duration_ms.