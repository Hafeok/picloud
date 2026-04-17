---
id: TC-314
title: Parquet store exit — hourly partitioned trace files written
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc314_parquet_store_exit_hourly_partitioned_trace_files_written"
validates:
  features: [FT-044]
  adrs: [ADR-046]
phase: 2
last-run: 2026-04-17T13:58:24.426609571+00:00
last-run-duration: 0.9s
---

## Description

End-to-end exit criteria verifying that the Parquet telemetry store produces valid,
queryable, hourly-partitioned Parquet files for both traces and metrics. Tests data
persistence across store lifetimes (drop and re-create), partition directory naming
per ADR-046 format, Parquet file validity, per-hour query isolation, metric round-trip
integrity, and empty time-range queries returning cleanly.