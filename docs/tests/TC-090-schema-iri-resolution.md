---
id: TC-090
title: schema_iri_resolution
type: scenario
status: passing
validates:
  features:
  - FT-002
  adrs:
  - ADR-031
phase: 1
---

emit a platform event (e.g. `ResourceReady`). Extract the `schema` field from the event envelope. GET the schema IRI. Assert 200 and a valid JSON Schema body.