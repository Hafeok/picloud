---
id: TC-051
title: GET /.well-known/openid-configuration
type: scenario
status: failing
validates:
  features:
  - FT-003
  adrs:
  - ADR-017
phase: 1
runner: picloud-test
runner-args: "oidc-authorization-code"
---

assert all required OpenID Connect Discovery fields present. Assert `issuer` value matches cluster domain exactly.