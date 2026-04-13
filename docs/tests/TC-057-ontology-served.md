---
id: TC-057
title: ontology_served
type: scenario
status: unimplemented
validates:
  features:
  - FT-008
  adrs:
  - ADR-019
phase: 1
---

GET the product's ontology IRI. Assert 200 with `text/turtle` content type and non-empty Turtle body containing the declared ontology.