---
id: TC-048
title: oidc_authorization_code
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-017
phase: 1
runner: cargo-test
runner-args: "tc048_oidc_authorization_code"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

initiate OIDC authorization code flow against a deployed Product. Complete passkey authentication. Assert ID token received with correct `iss`, `aud`, `sub`, and `exp` claims.