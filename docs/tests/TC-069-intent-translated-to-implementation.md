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
runner: scripts/run-tc.sh
runner-args: "intent-translated-to-implementation"
last-run: 2026-04-13T19:41:49.618598309+00:00
---

query `picloud:replicationFactor` and `picloud:replicationNodes` on the volume IRI after allocation. Assert both match the cluster's current node count.