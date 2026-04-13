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
last-run: 2026-04-13T21:37:33.242635225+00:00
---

GET the product's ontology IRI. Assert 200 with `text/turtle` content type and non-empty Turtle body containing the declared ontology.