---
id: TC-080
title: sparql_direct_mtls
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-027
phase: 1
runner: cargo-test
runner-args: "tc080_sparql_direct_mtls"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

query a product SPARQL endpoint directly from a workload using its injected mTLS certificate (no platform proxy hop). Assert 200 and correct query results.