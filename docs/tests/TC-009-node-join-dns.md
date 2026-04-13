---
id: TC-009
title: node_join_dns
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-003
phase: 1
runner: cargo-test
runner-args: "tc009_node_join_dns"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

after a third node joins, assert its hostname (`{node-id}.picloud.local`) resolves within 60 seconds of the `NodeJoined` event appearing in the RDF graph.