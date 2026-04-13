---
id: TC-227
title: User authenticates against Product-hosted application via OIDC
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc227_user_authenticates_against_product_hosted_application_via_oidc"
validates:
  features: [FT-025]
  adrs: [ADR-017, ADR-025, ADR-051]
phase: 2
last-run: 2026-04-13T22:03:38.656066583+00:00
---

## Description

End-to-end exit-criteria test for FT-025 (Product-scoped IAM and role assignment).

Validates the full OIDC authentication flow for a product-hosted application:

1. **Setup** — Register a human identity with platform roles (`editor`, `user`), register a passkey, and create a Product app registration (OIDC client) for `photo-app` with scopes `openid`, `photos:read`, `photos:write`.
2. **OIDC Discovery** — Verify the OIDC discovery document advertises the `authorization_code` grant type, correct endpoints, and JWKS key metadata.
3. **Authorization Code Flow** — User authenticates via passkey, the platform issues an authorization code for the product's OIDC client, and the code is exchanged for a product-scoped access token with correct scopes.
4. **Token Validation** — The access token carries the correct audience (product IRI `https://picloud.local/products/photo-app`), requested scopes (`openid`, `photos:read`), and the user's RBAC roles (`editor`, `user`).
5. **Token Exchange (RFC 8693)** — A platform token is exchanged for a product-scoped token via the on-behalf-of flow, preserving the original identity as the `actor` claim and targeting the product audience.
6. **Client Credentials with Audience + M2M Enforcement** — Self-scoped client credentials tokens work without M2M permissions. Cross-product access is blocked without an M2M permission declaration, succeeds after registering one, and fails again when requesting unauthorized scopes.