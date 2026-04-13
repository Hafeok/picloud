---
id: TC-104
title: shadow_swap_live_traffic
type: scenario
status: failing
validates:
  features:
  - FT-002
  adrs:
  - ADR-035
phase: 1
runner: picloud-test
runner-args: "shadow-swap-live-traffic"
---

trigger a platform replay while the cluster is serving live SPARQL queries (load: 10 queries/second). Assert zero query errors during replay. Assert the shadow swap is atomic — no queries return partial state.