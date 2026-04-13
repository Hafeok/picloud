---
id: TC-049
title: oidc_client_credentials
type: scenario
status: failing
validates:
  features:
  - FT-003
  adrs:
  - ADR-017
phase: 1
runner: picloud-test
runner-args: "oidc-client-credentials"
---

execute client credentials grant for an App Registration. Assert access token received, token type is Bearer, and `expires_in` is present.