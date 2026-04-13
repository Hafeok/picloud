---
id: TC-179
title: dns_nxdomain
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-052
phase: 1
runner: cargo-test
runner-args: "tc179_dns_nxdomain"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

query a hostname that does not exist in the cluster. Assert NXDOMAIN response with no fallthrough.