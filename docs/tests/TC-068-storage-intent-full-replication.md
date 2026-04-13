---
id: TC-068
title: storage_intent_full_replication
type: scenario
status: failing
validates:
  features:
  - FT-004
  adrs:
  - ADR-024
phase: 1
runner: picloud-test
runner-args: "storage-intent-full-replication"
---

declare a volume with `durability: full-replication`. Apply it. Query the RDF graph and assert the volume's replication state shows N replicas for an N-node cluster.