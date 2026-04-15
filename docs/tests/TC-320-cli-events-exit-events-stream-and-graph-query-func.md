---
id: TC-320
title: CLI events exit — events stream and graph query functional
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc320_cli_events_exit_events_stream_and_graph_query_functional"
validates:
  features: [FT-050]
  adrs: []
phase: 2
last-run: 2026-04-15T12:28:36.188972058+00:00
last-run-duration: 0.6s
---

## Description

Exit criteria verifying that all four CLI command families (events stream,
graph query, identity token, telemetry query) are fully functional. Tests
all parameter combinations, signal types, filter permutations, SSE event
parsing for a multi-event sequence, URL encoding of special characters,
device-flow round-trip (begin → pending → complete/expired), and SQL WHERE
clause parsing used by the telemetry subsystem.