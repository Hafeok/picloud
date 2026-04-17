---
id: TC-350
title: Telemetry SQL query endpoint returns 200 on simple parquet read
type: scenario
status: passing
runner: cargo-test
runner-args: "tc350_telemetry_sql_query_endpoint_returns_200"
validates:
  features: [FT-044, FT-045]
  adrs: []
phase: 4
last-run: 2026-04-17T14:25:33.709220581+00:00
last-run-duration: 0.8s
---

## Description

Regression guard for the `parquet-write-read` E2E scenario, which failed on
the Pi 5 cluster (2026-04-17) with `telemetry SQL query endpoint returned
status 500 Internal Server Error`.

**Invariant under test:** after the platform has accepted OTel data and
written at least one Parquet partition, a simple SQL query through the
telemetry endpoint (`picloud telemetry query` / HTTP API) returns 200 with
valid rows — never 500.

**Shape of the Rust test:**

1. Spin up a single-node `picloud-server` in a temp dir.
2. Accept a batch of OTel metric points via the ingestion endpoint.
3. Wait for the aggregator to flush (or force a flush).
4. Issue a trivial query such as `SELECT COUNT(*) FROM metrics` via the
   DataFusion SQL endpoint.
5. Assert HTTP 200 and at least one row. Non-200 or an error in the body
   must fail the test.

Capture the server-side error body on failure so the root cause (missing
partition, schema mismatch, DataFusion panic, etc.) surfaces in CI logs.