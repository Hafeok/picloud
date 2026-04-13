---
id: TC-295
title: IAM exit — passkey login, token issuance, RBAC enforcement functional
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc295_iam_exit_passkey_login_token_issuance_rbac_enforcement_functional"
validates:
  features: [FT-017]
  adrs: [ADR-017, ADR-025, ADR-026, ADR-051]
phase: 1
last-run: 2026-04-13T21:03:47.267771483+00:00
---

## Description

Exit criteria gate for FT-017. Validates all three IAM pillars work end-to-end:

1. **Passkey login** — human identity authenticates via FIDO2/WebAuthn passkey ceremony
2. **Token issuance** — platform-scoped and product-scoped tokens carry correct claims (identity, roles, audience)
3. **RBAC enforcement** — different roles produce different token scopes; restricted users cannot access operator/admin roles
4. **Workload identity** — workload certificates and tokens are issued correctly
5. **Token expiry** — expired tokens are rejected