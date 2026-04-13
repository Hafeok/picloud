---
id: TC-171
title: role_inheritance_claims
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-051
phase: 1
runner: cargo-test
runner-args: "tc171_role_inheritance_claims"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

assign the `editor` role (which inherits `viewer`). Issue a token. Assert the token's `permissions` array contains both `editor`-level and `viewer`-level permissions (transitive inheritance via OWL inference).