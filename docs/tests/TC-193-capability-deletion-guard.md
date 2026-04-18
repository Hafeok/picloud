---
id: TC-193
title: capability_deletion_guard
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-055
phase: 1
runner: cargo-test
runner-args: "capability_deletion_guard"
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 1.5s
---

attempt `picloud resource delete capability/gps-to-place` while `maps-app` declares a dependency on it. Assert the delete is rejected with a dependency error. Assert the capability remains in the cluster graph.