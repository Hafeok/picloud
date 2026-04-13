---
id: TC-152
title: parquet_portability
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-046
phase: 1
---

copy a Parquet file off-node. Open it with `pyarrow` on an external machine. Assert the schema and data are readable without any PiCloud tools.