---
id: TC-009
title: node_join_dns
type: scenario
status: unimplemented
validates:
  features:
  - FT-006
  adrs:
  - ADR-003
phase: 1
---

after a third node joins, assert its hostname (`{node-id}.picloud.local`) resolves within 60 seconds of the `NodeJoined` event appearing in the RDF graph.