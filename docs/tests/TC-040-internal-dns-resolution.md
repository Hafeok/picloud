---
id: TC-040
title: internal_dns_resolution
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-014
phase: 1
runner: cargo-test
runner-args: "tc040_internal_dns_resolution"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

deploy two containers in the same product. From container A, resolve `{resource-B}.{product}.picloud.internal`. Assert it resolves to container B's IP within 10 seconds of `ResourceReady`.