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
runner: scripts/run-tc.sh
runner-args: "partial-failure-reapply"
last-run: 2026-04-17T19:13:00.299404881+00:00
last-run-duration: 0.0s
---

kill the cluster midway through a `resource apply`. Re-apply after recovery. Assert the final state is correct and no resources are duplicated.