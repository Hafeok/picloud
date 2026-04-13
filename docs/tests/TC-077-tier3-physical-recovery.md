---
id: TC-077
title: tier3_physical_recovery
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-026
phase: 1
runner: cargo-test
runner-args: "tc077_tier3_physical_recovery"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

simulate all admin accounts being inaccessible. Run `picloud cluster recover` directly on a node (local-only, no network). Assert a new bootstrap token is generated and the recovery event appears as a high-severity audit entry in the platform event log.