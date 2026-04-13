---
id: TC-112
title: group_membership_via_inference
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-037
phase: 1
---

create a user, add tag `team:backend`. Assert a `GroupMembershipChanged` event is emitted and the user appears as `picloud:hasMember` on the `backend-developers` group within one event cycle (< 2 seconds).