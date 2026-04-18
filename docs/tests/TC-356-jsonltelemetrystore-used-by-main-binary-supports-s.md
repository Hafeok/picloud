---
id: TC-356
title: JsonlTelemetryStore used by main binary supports SQL queries
type: scenario
status: passing
runner: cargo-test
runner-args: "tc356_main_binary_telemetry_backend_supports_sql"
validates:
  features:
  - FT-044
  - FT-045
  adrs:
  - ADR-046
phase: 3
last-run: 2026-04-18T15:52:21.360416322+00:00
last-run-duration: 0.7s
---

## Description

Regression guard for the `parquet-write-read` E2E scenario, which failed on
the Pi 5 cluster (2026-04-18) with
`telemetry SQL query endpoint returned status 500 Internal Server Error`.

Direct curl against the live cluster surfaced the underlying cause:

```
{"error":"SQL query failed: Telemetry query failed:
         SQL queries not supported by this telemetry backend"}
```

TC-350 already guards DataFusion SQL over `ParquetTelemetryStore`, but
`src/main.rs:1020` wires `JsonlTelemetryStore` into the live binary.
`JsonlTelemetryStore` does not override the default `query_sql` trait
method in `picloud-domain/src/traits.rs:642-650`, which returns the
"not supported" error. The TC-350 green result therefore does not reflect
what the real server does — it tests the wrong backend.

**Invariant under test:** whatever `TelemetryStore` implementation the
composition root (`src/main.rs`) instantiates MUST answer
`/api/telemetry/query` with HTTP 200 for a trivial `SELECT COUNT(*) FROM
metrics` after ingestion — never 500 with "SQL queries not supported by
this telemetry backend".

**Shape of the Rust test:**

1. Import the telemetry-store factory the composition root uses (or move
   that factory into a small helper so the test can call exactly the same
   constructor as `main`).
2. Build an in-process `PiCloudHttpServer` with that backend, identical to
   the one on the cluster.
3. POST a small OTel metric batch to `/otel`.
4. POST `{"sql":"SELECT COUNT(*) AS total FROM metrics","signal":"metrics"}`
   to `/api/telemetry/query`.
5. Assert HTTP 200 and at least one row; surface the response body on
   failure.
6. Also POST `SELECT COUNT(*) FROM traces` (the exact query the E2E
   scenario uses) and assert HTTP 200 — the Pi 5 scenario hit this path
   with no data and got 500.

If the fix is to switch the default backend to `ParquetTelemetryStore`,
this TC also effectively retires the Jsonl-only code path for the main
binary. If the fix is to give `JsonlTelemetryStore` a SQL implementation,
the same assertions hold.