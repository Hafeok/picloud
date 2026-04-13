---
id: TC-238
title: IAM token issued after passkey authentication grants RBAC-scoped access
type: scenario
status: passing
runner: cargo-test
runner-args: "tc238_iam_token_issued_after_passkey_authentication_grants_rbac_scoped_access"
validates:
  features: [FT-017]
  adrs: [ADR-025, ADR-051]
phase: 1
last-run: 2026-04-13T21:03:47.267771483+00:00
---

## Description

Verifies the complete passkey-to-RBAC-token flow: register human identities with
different RBAC roles, register passkeys (FIDO2), authenticate via passkey ceremony,
and confirm the issued token carries the correct RBAC roles and product-scoped
audience. Ensures role separation between identities (admin vs viewer) and that
product-scoped tokens include the correct audience claim.