---
id: TC-122
title: ontology_deploy_immediate
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-039
phase: 1
runner: cargo-test
runner-args: "ontology_deploy_immediate"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

deploy a product with a new subclass declaration. Assert the inference is materialised and queryable within 5 seconds of `ProductDeployed` event.