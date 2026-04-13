---
id: TC-162
title: sni_cert_selection
type: scenario
status: failing
validates:
  features:
  - FT-006
  adrs:
  - ADR-048
phase: 1
runner: picloud-test
runner-args: "sni-cert-selection"
---

declare two ingresses for two different hostnames. Connect to each hostname. Assert each connection receives the correct TLS certificate (SNI-based selection working).