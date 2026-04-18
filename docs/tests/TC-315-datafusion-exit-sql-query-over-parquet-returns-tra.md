---
id: TC-315
title: DataFusion exit — SQL query over Parquet returns traces
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc315_datafusion_exit_sql_query_over_parquet_returns_traces"
validates:
  features: [FT-045]
  adrs: [ADR-046]
phase: 2
last-run: 2026-04-18T11:08:54.380471473+00:00
last-run-duration: 0.7s
---

## Description

End-to-end exit-criteria test verifying that DataFusion SQL queries over the
Parquet telemetry store produce correct, complete results after a store
recreation (persistence check). Validates:

1. 50 spans written and queryable via `SELECT *`
2. Row structure is well-formed JSON with expected columns
3. `WHERE service_name` filtering returns correct subset
4. Complex multi-condition `WHERE` with `AND`
5. Metrics SQL query works for both `SELECT *` and filtered
6. `ORDER BY value DESC` ordering
7. `GROUP BY` with `HAVING` and `AVG` aggregation
8. Data survives store drop and recreation (Parquet persistence)