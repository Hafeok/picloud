---
id: TC-108
title: tag_rdf_projection
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-036
phase: 1
---

add a tag. Query the resource IRI via SPARQL. Assert the `picloud:tag` triple with correct `picloud:tagKey` and `picloud:tagValue` is present within the projection latency budget.