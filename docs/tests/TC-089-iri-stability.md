---
id: TC-089
title: iri_stability
type: scenario
status: passing
validates:
  features: []
  adrs:
  - ADR-029
phase: 1
---

apply a container resource, record its IRI. Reschedule the container to a different node. Assert the IRI is unchanged and still dereferenceable.