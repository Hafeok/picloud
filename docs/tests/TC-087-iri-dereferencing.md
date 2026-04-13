---
id: TC-087
title: iri_dereferencing
type: scenario
status: passing
validates:
  features: []
  adrs:
  - ADR-029
phase: 1
runner: picloud-test
runner-args: "iri-dereferencing"
---

GET the IRI of every known resource type (cluster root, node, product, container, volume, identity). Assert 200 and non-empty body for each content type.