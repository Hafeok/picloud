---
id: TC-114
title: circular_group_rejection
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-037
phase: 1
---

attempt to create a group membership rule where group A contains group B and group B contains group A. Assert the platform rejects the cycle at resource apply time.