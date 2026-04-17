---
id: TC-114
title: circular_group_rejection
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-037
phase: 1
runner: cargo-test
runner-args: "circular_group_rejection"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.8s
failure-message: "No matching test function found (0 tests ran)"
---

attempt to create a group membership rule where group A contains group B and group B contains group A. Assert the platform rejects the cycle at resource apply time.