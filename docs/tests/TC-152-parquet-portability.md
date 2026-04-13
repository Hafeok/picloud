---
id: TC-152
title: parquet_portability
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-046
phase: 1
runner: picloud-test
runner-args: "parquet-portability"
---

copy a Parquet file off-node. Open it with `pyarrow` on an external machine. Assert the schema and data are readable without any PiCloud tools.