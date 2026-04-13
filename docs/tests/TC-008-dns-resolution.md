---
id: TC-008
title: dns_resolution
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-003
phase: 1
runner: cargo-test
runner-args: "tc008_dns_resolution"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

after `cluster init`, assert `picloud.local` resolves to a cluster node IP from an external client on the same broadcast domain within 2 seconds.