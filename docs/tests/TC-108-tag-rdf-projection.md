---
id: TC-108
title: tag_rdf_projection
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-036
phase: 1
runner: cargo-test
runner-args: "tag_rdf_projection"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 1.2s
failure-message: "No matching test function found (0 tests ran)"
---

add a tag. Query the resource IRI via SPARQL. Assert the `picloud:tag` triple with correct `picloud:tagKey` and `picloud:tagValue` is present within the projection latency budget.