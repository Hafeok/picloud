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
runner: scripts/run-tc.sh
runner-args: "replication-on-node-join"
last-run: 2026-04-13T19:41:49.618598309+00:00
---

allocate a volume on a two-node cluster. Add a third node. Assert the volume is replicated to the new node within 120 seconds of the `NodeJoined` event.