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
runner: picloud-test
runner-args: "group-role-inheritance"
---

assign a group to a role, assert all group members receive the role's permissions in their tokens.