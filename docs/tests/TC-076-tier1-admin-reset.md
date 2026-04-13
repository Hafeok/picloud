---
id: TC-076
title: tier1_admin_reset
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-026
phase: 1
runner: cargo-test
runner-args: "tc076_tier1_admin_reset"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

admin A initiates a passkey reset for user B via `picloud identity reset-passkey`. User B re-enrolls. Assert old passkey is revoked (old credential rejected), new passkey accepted.