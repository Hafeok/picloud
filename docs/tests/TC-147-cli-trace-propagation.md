---
id: TC-147
title: cli_trace_propagation
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-045
phase: 1
runner: cargo-test
runner-args: "cli_trace_propagation"
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 1.6s
---

run `picloud resource apply`. Query the Parquet store for the trace ID from the CLI output. Assert end-to-end spans: CLI root → Raft append → RDF projection → workload start.