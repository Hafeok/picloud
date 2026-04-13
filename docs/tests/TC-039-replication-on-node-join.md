---
id: TC-039
title: replication_on_node_join
type: scenario
status: failing
validates:
  features:
  - FT-004
  adrs:
  - ADR-013
phase: 1
runner: picloud-test
runner-args: "replication-on-node-join"
---

allocate a volume on a two-node cluster. Add a third node. Assert the volume is replicated to the new node within 120 seconds of the `NodeJoined` event.