---
id: TC-076
title: tier1_admin_reset
type: scenario
status: failing
validates:
  features:
  - FT-003
  adrs:
  - ADR-026
phase: 1
runner: picloud-test
runner-args: "tier1-admin-reset"
---

admin A initiates a passkey reset for user B via `picloud identity reset-passkey`. User B re-enrolls. Assert old passkey is revoked (old credential rejected), new passkey accepted.