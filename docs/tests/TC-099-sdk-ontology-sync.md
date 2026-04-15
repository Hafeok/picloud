---
id: TC-099
title: sdk_ontology_sync
type: scenario
status: passing
validates:
  features:
  - FT-010
  adrs:
  - ADR-033
phase: 1
runner: picloud-test
runner-args: run --scenario sdk-ontology-sync
last-run: 2026-04-15T12:41:47.468906701+00:00
last-run-duration: 0.0s
---

add a new resource type to the platform ontology. Re-run `picloud sdk generate`. Assert the new type appears in all three generated SDKs with correct property types.