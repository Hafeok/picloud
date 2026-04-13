---
id: TC-162
title: sni_cert_selection
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-048
phase: 1
runner: cargo-test
runner-args: "tc162_sni_cert_selection"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

declare two ingresses for two different hostnames. Connect to each hostname. Assert each connection receives the correct TLS certificate (SNI-based selection working).