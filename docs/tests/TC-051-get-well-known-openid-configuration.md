---
id: TC-051
title: GET /.well-known/openid-configuration
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-017
phase: 1
runner: cargo-test
runner-args: "tc051_openid_configuration"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

assert all required OpenID Connect Discovery fields present. Assert `issuer` value matches cluster domain exactly.