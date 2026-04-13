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
---

assign the `editor` role (which inherits `viewer`). Issue a token. Assert the token's `permissions` array contains both `editor`-level and `viewer`-level permissions (transitive inheritance via OWL inference).