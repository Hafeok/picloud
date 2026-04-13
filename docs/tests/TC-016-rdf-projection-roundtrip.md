---
id: TC-016
title: rdf_projection_roundtrip
type: scenario
status: passing
validates:
  features:
  - FT-002
  adrs:
  - ADR-005
phase: 1
---

apply a product with containers, volumes, and identities. Assert every declared resource appears as typed triples in the graph via SPARQL ASK. Wipe Oxigraph, replay the event log, assert the graph is identical.