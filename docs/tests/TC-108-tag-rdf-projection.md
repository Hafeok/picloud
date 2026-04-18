---
id: TC-108
title: tag_rdf_projection
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-036
phase: 1
runner: cargo-test
runner-args: "tag_rdf_projection"
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 0.9s
---

add a tag. Query the resource IRI via SPARQL. Assert the `picloud:tag` triple with correct `picloud:tagKey` and `picloud:tagValue` is present within the projection latency budget.