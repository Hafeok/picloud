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
runner: cargo-test
runner-args: "parquet_portability"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

copy a Parquet file off-node. Open it with `pyarrow` on an external machine. Assert the schema and data are readable without any PiCloud tools.