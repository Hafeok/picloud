---
id: TC-114
title: circular_group_rejection
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-037
phase: 1
runner: cargo-test
runner-args: "circular_group_rejection"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

attempt to create a group membership rule where group A contains group B and group B contains group A. Assert the platform rejects the cycle at resource apply time.