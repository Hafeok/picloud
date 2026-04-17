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
runner: cargo-test
runner-args: "tag_sparql_queryable"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 1.2s
failure-message: "No matching test function found (0 tests ran)"
---

run `picloud tag find environment=production`. Assert all tagged resources returned. Run the equivalent SPARQL query directly and assert identical results.