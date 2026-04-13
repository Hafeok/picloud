---
id: TC-084
title: platform_ca_export
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-030
phase: 1
runner: cargo-test
runner-args: "tc084_platform_ca_export"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

run `picloud ca export`. Trust the exported CA in a test client's OS trust store. Connect to `https://picloud.local`. Assert 200 with no TLS warning.