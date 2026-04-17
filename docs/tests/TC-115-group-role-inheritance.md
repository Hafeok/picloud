---
id: TC-115
title: group_role_inheritance
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-037
phase: 1
runner: cargo-test
runner-args: "group_role_inheritance"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.8s
failure-message: "No matching test function found (0 tests ran)"
---

assign a group to a role, assert all group members receive the role's permissions in their tokens.