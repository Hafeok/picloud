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
last-run: 2026-04-13T21:47:42.689812716+00:00
---

assign a group to a role, assert all group members receive the role's permissions in their tokens.