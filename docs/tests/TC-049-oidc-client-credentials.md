---
id: TC-049
title: oidc_client_credentials
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-017
phase: 1
runner: cargo-test
runner-args: "tc049_oidc_client_credentials"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

execute client credentials grant for an App Registration. Assert access token received, token type is Bearer, and `expires_in` is present.