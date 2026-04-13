---
id: TC-069
title: intent_translated_to_implementation
type: scenario
status: passing
validates:
  features:
  - FT-004
  adrs:
  - ADR-024
phase: 1
---

query `picloud:replicationFactor` and `picloud:replicationNodes` on the volume IRI after allocation. Assert both match the cluster's current node count.