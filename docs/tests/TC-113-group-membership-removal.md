---
id: TC-113
title: group_membership_removal
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-037
phase: 1
runner: picloud-test
runner-args: "group-membership-removal"
---

remove the `team:backend` tag. Assert the membership triple is retracted from the graph and the user's next issued token lacks the `product-developer` role.