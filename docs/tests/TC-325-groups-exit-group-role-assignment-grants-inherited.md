---
id: TC-325
title: Groups exit — group role assignment grants inherited permissions
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc325_groups_exit_group_role_assignment_grants_inherited_permissions"
validates:
  features: [FT-056]
  adrs: []
phase: 1
last-run: 2026-04-17T07:02:16.701739289+00:00
last-run-duration: 0.6s
---

## Description

Exit criteria: Validates the full group role inheritance system end-to-end.
Covers multiple groups with multiple members, users in multiple groups,
combination of direct roles and group-inherited roles, dynamic role assignment
and revocation on groups, membership removal, issue_token / issue_token_with_audience /
resolve_roles all correctly include group-inherited roles, idempotent membership
operations, and error handling for non-existent groups.