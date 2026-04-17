---
id: TC-293
title: mDNS discovery exit — all nodes discovered and joined within timeout
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc293_mdns_discovery_exit_all_nodes_discovered_and_joined_within_timeout"
validates:
  features: [FT-013]
  adrs: []
phase: 1
last-run: 2026-04-17T15:53:21.715134689+00:00
last-run-duration: 1.1s
---

## Description

Start N nodes with staggered boot (200ms apart) and verify the full discovery mesh (every node sees all others) is established within the timeout. Then shut down one node and verify remaining nodes detect the departure. This is the exit criterion for mDNS node discovery — the feature is complete when all nodes can discover and join the cluster within the allotted time.