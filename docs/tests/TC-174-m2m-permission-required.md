---
id: TC-174
title: m2m_permission_required
type: scenario
status: failing
validates:
  features:
  - FT-003
  adrs:
  - ADR-051
phase: 1
runner: picloud-test
runner-args: "m2m-permission-required"
---

attempt M2M client credentials from `photo-app` to `user-service` without an `m2m-permission` resource in `user-service`. Assert 403 and a clear error.