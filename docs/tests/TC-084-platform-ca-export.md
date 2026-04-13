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
---

run `picloud ca export`. Trust the exported CA in a test client's OS trust store. Connect to `https://picloud.local`. Assert 200 with no TLS warning.