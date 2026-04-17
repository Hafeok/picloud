---
id: TC-066
title: ontology_version_binding
type: scenario
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-023
phase: 1
runner: scripts/run-tc.sh
runner-args: "ontology-version-binding"
last-run: 2026-04-17T14:18:42.769141632+00:00
last-run-duration: 0.0s
---

deploy product v1 with an ontology resource. Assert the ontology IRI is versioned (`/ontology/v1`) and resolves with the correct Turtle body. Deploy v2 with an updated ontology. Assert the v2 IRI resolves with the new body and the v1 IRI still resolves with the original body.