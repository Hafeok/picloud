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
---

query a product SPARQL endpoint directly from a workload using its injected mTLS certificate (no platform proxy hop). Assert 200 and correct query results.