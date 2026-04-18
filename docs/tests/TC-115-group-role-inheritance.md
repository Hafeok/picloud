---
id: TC-115
title: group_role_inheritance
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-037
phase: 1
runner: cargo-test
runner-args: "group_role_inheritance"
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 1.1s
---

assign a group to a role, assert all group members receive the role's permissions in their tokens.