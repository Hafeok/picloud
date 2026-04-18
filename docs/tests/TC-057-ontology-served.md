---
id: TC-057
title: ontology_served
type: scenario
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-019
phase: 1
runner: scripts/run-tc.sh
runner-args: "ontology-served"
last-run: 2026-04-18T11:08:48.461897691+00:00
last-run-duration: 0.0s
---

GET the product's ontology IRI. Assert 200 with `text/turtle` content type and non-empty Turtle body containing the declared ontology.