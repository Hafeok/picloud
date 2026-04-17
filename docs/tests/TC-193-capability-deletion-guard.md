---
id: TC-193
title: capability_deletion_guard
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-055
phase: 1
runner: cargo-test
runner-args: "capability_deletion_guard"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 1.0s
failure-message: "No matching test function found (0 tests ran)"
---

attempt `picloud resource delete capability/gps-to-place` while `maps-app` declares a dependency on it. Assert the delete is rejected with a dependency error. Assert the capability remains in the cluster graph.