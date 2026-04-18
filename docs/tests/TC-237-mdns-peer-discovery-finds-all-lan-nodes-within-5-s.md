---
id: TC-237
title: mDNS peer discovery finds all LAN nodes within 5 seconds
type: scenario
status: passing
runner: cargo-test
runner-args: "tc237_mdns_peer_discovery_finds_all_lan_nodes_within_5_seconds"
validates:
  features: [FT-013]
  adrs: []
phase: 1
last-run: 2026-04-18T18:01:49.656428213+00:00
last-run-duration: 3.7s
---

## Description

Spin up three mDNS nodes on localhost, each with its own peer list. Start browsing on all three nodes simultaneously. Assert that every node discovers the other two within 5 seconds. Verify correct peer info (node ID, port, address) is propagated through the mDNS TXT records.