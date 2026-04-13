---
id: TC-043
title: partial_failure_reapply
type: scenario
status: passing
validates:
  features:
  - FT-001
  - FT-007
  adrs:
  - ADR-015
phase: 1
---

kill the cluster midway through a `resource apply`. Re-apply after recovery. Assert the final state is correct and no resources are duplicated.