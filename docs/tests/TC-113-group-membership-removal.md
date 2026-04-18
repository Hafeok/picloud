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
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 0.7s
---

remove the `team:backend` tag. Assert the membership triple is retracted from the graph and the user's next issued token lacks the `product-developer` role.