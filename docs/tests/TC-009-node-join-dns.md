---
id: TC-009
title: node_join_dns
type: scenario
status: failing
validates:
  features:
  - FT-006
  adrs:
  - ADR-003
phase: 1
runner: picloud-test
runner-args: "node-join-dns"
---

after a third node joins, assert its hostname (`{node-id}.picloud.local`) resolves within 60 seconds of the `NodeJoined` event appearing in the RDF graph.