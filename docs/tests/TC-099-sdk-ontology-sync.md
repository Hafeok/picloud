---
id: TC-099
title: sdk_ontology_sync
type: scenario
status: failing
validates:
  features:
  - FT-010
  adrs:
  - ADR-033
phase: 1
runner: picloud-test
runner-args: "sdk-ontology-sync"
---

add a new resource type to the platform ontology. Re-run `picloud sdk generate`. Assert the new type appears in all three generated SDKs with correct property types.