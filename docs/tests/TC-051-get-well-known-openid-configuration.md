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
---

assert all required OpenID Connect Discovery fields present. Assert `issuer` value matches cluster domain exactly.