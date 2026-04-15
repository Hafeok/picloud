---
id: TC-268
title: Group with assigned role grants inherited permissions to member users
type: scenario
status: passing
runner: cargo-test
runner-args: "tc268_group_with_assigned_role_grants_inherited_permissions_to_member_users"
validates:
  features: [FT-056]
  adrs: []
phase: 1
last-run: 2026-04-15T13:38:47.319051934+00:00
last-run-duration: 0.5s
---

## Description

Verifies that when a role is assigned to a group, all users who are members
of that group inherit the role. Covers: group creation with roles, membership
management, role inheritance in issued tokens and resolve_roles, removal of
membership (and its effect on inherited roles), and dynamic role assignment
to groups.