---
id: TC-178
title: dns_cache_invalidation
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-052
phase: 1
runner: cargo-test
runner-args: "tc178_dns_cache_invalidation"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

reschedule a container workload to a different node. Assert the DNS A record for the workload's ingress hostname updates to the new node's IP within 30 seconds (well before TTL expiry).