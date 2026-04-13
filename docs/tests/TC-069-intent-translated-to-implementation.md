---
id: TC-069
title: intent_translated_to_implementation
type: scenario
status: failing
validates:
  features:
  - FT-004
  adrs:
  - ADR-024
phase: 1
runner: picloud-test
runner-args: "intent-translated-to-implementation"
---

query `picloud:replicationFactor` and `picloud:replicationNodes` on the volume IRI after allocation. Assert both match the cluster's current node count.