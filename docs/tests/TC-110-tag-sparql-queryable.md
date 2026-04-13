---
id: TC-110
title: tag_sparql_queryable
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-036
phase: 1
---

run `picloud tag find environment=production`. Assert all tagged resources returned. Run the equivalent SPARQL query directly and assert identical results.