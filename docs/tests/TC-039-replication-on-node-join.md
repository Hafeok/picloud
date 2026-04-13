---
id: TC-039
title: replication_on_node_join
type: scenario
status: passing
validates:
  features:
  - FT-004
  adrs:
  - ADR-013
phase: 1
---

allocate a volume on a two-node cluster. Add a third node. Assert the volume is replicated to the new node within 120 seconds of the `NodeJoined` event.