---
id: TC-278
title: Cross-product graph access returns 403 for non-owner non-admin
type: scenario
status: passing
runner: cargo-test
runner-args: "tc278_cross_product_graph_access_returns_403_for_non_owner_non_admin"
validates:
  features: [FT-074]
  adrs: []
phase: 3
last-run: 2026-04-15T16:21:44.576829893+00:00
last-run-duration: 0.5s
---

## Description

Verifies that a non-owner, non-admin user is blocked (HTTP 403) when attempting
to access another product's SPARQL graph endpoint. Three scenarios are exercised:

1. **Non-owner non-admin with cross-product token → 403**: A regular user whose token
   is scoped to product A tries to query product B's graph and receives 403 Forbidden
   with an `invalid_audience` error body.
2. **Product owner (matching audience) → allowed**: The same user with a token scoped
   to the target product is permitted through.
3. **Platform admin with cross-product token → allowed**: An admin-role user whose
   token is scoped to a different product is still permitted because platform admins
   bypass the audience check (FT-074).