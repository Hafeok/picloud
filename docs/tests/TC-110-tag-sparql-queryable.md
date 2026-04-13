---
id: TC-110
title: tag_sparql_queryable
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-036
phase: 1
runner: picloud-test
runner-args: "tag-sparql-queryable"
---

run `picloud tag find environment=production`. Assert all tagged resources returned. Run the equivalent SPARQL query directly and assert identical results.