---
id: TC-048
title: oidc_authorization_code
type: scenario
status: unimplemented
validates:
  features:
  - FT-003
  adrs:
  - ADR-017
phase: 1
---

initiate OIDC authorization code flow against a deployed Product. Complete passkey authentication. Assert ID token received with correct `iss`, `aud`, `sub`, and `exp` claims.