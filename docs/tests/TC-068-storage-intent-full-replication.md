---
id: TC-068
title: storage_intent_full_replication
type: scenario
status: passing
validates:
  features:
  - FT-004
  adrs:
  - ADR-024
phase: 1
runner: scripts/run-tc.sh
runner-args: "storage-intent-full-replication"
last-run: 2026-04-13T19:41:49.618598309+00:00
---

declare a volume with `durability: full-replication`. Apply it. Query the RDF graph and assert the volume's replication state shows N replicas for an N-node cluster.