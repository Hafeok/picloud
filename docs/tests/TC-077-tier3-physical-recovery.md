---
id: TC-077
title: tier3_physical_recovery
type: scenario
status: failing
validates:
  features:
  - FT-003
  adrs:
  - ADR-026
phase: 1
runner: picloud-test
runner-args: "tier3-physical-recovery"
---

simulate all admin accounts being inaccessible. Run `picloud cluster recover` directly on a node (local-only, no network). Assert a new bootstrap token is generated and the recovery event appears as a high-severity audit entry in the platform event log.