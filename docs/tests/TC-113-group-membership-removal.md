---
id: TC-113
title: group_membership_removal
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-037
phase: 1
runner: cargo-test
runner-args: "group_membership_removal"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

remove the `team:backend` tag. Assert the membership triple is retracted from the graph and the user's next issued token lacks the `product-developer` role.