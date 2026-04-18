---
id: TC-258
title: DataFusion SQL query returns traces from Parquet store
type: scenario
status: passing
runner: cargo-test
runner-args: "tc258_datafusion_sql_query_returns_traces_from_parquet_store"
validates:
  features: [FT-045]
  adrs: [ADR-046]
phase: 2
last-run: 2026-04-18T14:10:55.275365608+00:00
last-run-duration: 0.7s
---

## Description

Verifies that DataFusion can execute SQL queries over Parquet-stored telemetry
data and return correct, filtered results. Tests include:

1. `SELECT * FROM traces` — returns all rows
2. `WHERE service_name = 'api-server'` — string equality filter
3. `WHERE duration_ms > 100` — numeric comparison filter
4. `GROUP BY service_name` with `COUNT(*)` — aggregation
5. `ORDER BY duration_ms DESC LIMIT 2` — ordering and limit
6. Empty store returns empty result set
7. Metrics table SQL query with filter
8. Column names match the SQL projection