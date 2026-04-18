---
id: TC-355
title: Cluster node listing reflects all mDNS-discovered peers
type: scenario
status: passing
validates:
  features:
  - FT-013
  - FT-014
  adrs: []
phase: 4
runner: cargo-test
runner-args: tc355_cluster_node_listing_reflects_all_mdns_peers
last-run: 2026-04-18T18:01:56.030164399+00:00
last-run-duration: 0.8s
---

## Description

Regression guard for the cluster-formation gap observed on the Pi 5 cluster
(2026-04-17): with `picloud-server` running on both node3 (192.168.88.22,
leader) and worker02 (192.168.88.20), a `curl http://localhost:7443/` on
node3 reported only a single node (`"nodes": [{ "nodeId": ..., "name":
"node3" }]`). worker02 was not listed, despite answering `/health` and
being on the same subnet with mDNS reachable. Both servers were started
without an explicit join command.

**Invariant under test:** once two or more `picloud-server` processes are
running on the same subnet with default mDNS config, the cluster root
resource (`GET /`) on every node must list all peers within a bounded
discovery window (≤ 30s). Either the servers auto-form a Raft cluster via
mDNS (FT-013 + FT-014), or the response must explicitly indicate the
discovery pending state — never silently omit a live peer.

**Shape of the Rust test:**

1. Spin up two in-process `picloud-server` instances on loopback using
   disjoint data dirs and ports, both with mDNS enabled.
2. Poll `GET /` on each instance until both report two nodes or 30s elapse.
3. Assert both responses list both `nodeId`s and mark exactly one
   `isLeader: true`.
4. Parametrize to three nodes and repeat — ensures discovery scales beyond
   the pairwise case.