---
id: TC-092
title: schema_iri_permanence
type: scenario
status: unimplemented
validates:
  features:
  - FT-002
  adrs:
  - ADR-031
phase: 1
---

deploy a new platform version that introduces schema v2 for an event type. Assert the v1 schema IRI still resolves and returns the original v1 schema body.