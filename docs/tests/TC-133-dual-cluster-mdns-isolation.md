---
id: TC-133
title: dual_cluster_mDNS_isolation
type: scenario
status: failing
validates:
  features:
  - FT-007
  adrs:
  - ADR-042
phase: 1
runner: picloud-test
runner-args: "dual-cluster-mdns-isolation"
---

init two clusters on the same network with different domains (`picloud.local` and `lab.local`). Assert that nodes from cluster A do not appear in cluster B's node list (SPARQL query), and vice versa.