---
id: TC-110
title: tag_sparql_queryable
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-036
phase: 1
runner: cargo-test
runner-args: "tag_sparql_queryable"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

run `picloud tag find environment=production`. Assert all tagged resources returned. Run the equivalent SPARQL query directly and assert identical results.