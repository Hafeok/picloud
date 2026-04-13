---
id: TC-030
title: token_expiry_enforcement
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-009
phase: 1
runner: cargo-test
runner-args: "tc030_token_expiry_enforcement"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

issue a token, wait for it to expire, present it to an IAM-gated endpoint, assert 401 with `WWW-Authenticate` header.