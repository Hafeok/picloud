---
id: TC-263
title: CLI events stream and graph query return expected results
type: scenario
status: passing
runner: cargo-test
runner-args: "tc263_cli_events_stream_and_graph_query_return_expected_results"
validates:
  features: [FT-050]
  adrs: []
phase: 2
last-run: 2026-04-15T12:28:36.188972058+00:00
last-run-duration: 0.7s
---

## Description

Verifies that the CLI `events stream`, `graph query`, `identity token`, and
`telemetry query` commands produce correct URL paths, parse SSE events and
device-flow JSON correctly, URL-encode SPARQL queries, and format results
for display. Covers all parameter combinations for each command family.