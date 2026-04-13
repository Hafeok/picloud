---
id: TC-017
title: sparql_query_types
type: scenario
status: passing
validates:
  features:
  - FT-002
  adrs:
  - ADR-005
phase: 1
runner: cargo-test
runner-args: "tc017_sparql_query_types"
---

execute SELECT, ASK, CONSTRUCT, and DESCRIBE queries against the platform graph. Assert correct result formats and non-empty results for known-populated graphs.