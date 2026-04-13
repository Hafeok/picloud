---
id: TC-147
title: cli_trace_propagation
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-045
phase: 1
runner: picloud-test
runner-args: "cli-trace-propagation"
---

run `picloud resource apply`. Query the Parquet store for the trace ID from the CLI output. Assert end-to-end spans: CLI root → Raft append → RDF projection → workload start.