---
id: TC-122
title: ontology_deploy_immediate
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-039
phase: 1
runner: cargo-test
runner-args: "ontology_deploy_immediate"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.9s
failure-message: "No matching test function found (0 tests ran)"
---

deploy a product with a new subclass declaration. Assert the inference is materialised and queryable within 5 seconds of `ProductDeployed` event.