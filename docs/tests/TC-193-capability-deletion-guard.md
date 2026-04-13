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
last-run: 2026-04-13T21:47:42.689812716+00:00
---

attempt `picloud resource delete capability/gps-to-place` while `maps-app` declares a dependency on it. Assert the delete is rejected with a dependency error. Assert the capability remains in the cluster graph.