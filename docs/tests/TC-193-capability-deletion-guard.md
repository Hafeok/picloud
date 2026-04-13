---
id: TC-193
title: capability_deletion_guard
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-055
phase: 1
---

attempt `picloud resource delete capability/gps-to-place` while `maps-app` declares a dependency on it. Assert the delete is rejected with a dependency error. Assert the capability remains in the cluster graph.