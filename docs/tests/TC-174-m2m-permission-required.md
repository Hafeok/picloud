---
id: TC-174
title: m2m_permission_required
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-051
phase: 1
runner: cargo-test
runner-args: "tc174_m2m_permission_required"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

attempt M2M client credentials from `photo-app` to `user-service` without an `m2m-permission` resource in `user-service`. Assert 403 and a clear error.