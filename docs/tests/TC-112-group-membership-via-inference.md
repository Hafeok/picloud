---
id: TC-112
title: group_membership_via_inference
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-037
phase: 1
runner: cargo-test
runner-args: "group_membership_via_inference"
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 0.6s
---

create a user, add tag `team:backend`. Assert a `GroupMembershipChanged` event is emitted and the user appears as `picloud:hasMember` on the `backend-developers` group within one event cycle (< 2 seconds).